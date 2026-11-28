use std::{collections::VecDeque, ops::Range, sync::Arc};

use xrack_runtime::{
    scene_effect::CaptureRenderables,
    scene3d::{MeshCullMode, MeshDepthCoverage, MeshDepthDraw, MeshTopology},
};

use super::{
    DEPTH_FORMAT, MeshBuffer, append_floats, buffer,
    deform::{Deformation, DeformationBinding, DeformationLayouts, SKIN_LAYOUT},
    pipeline::{GEOMETRY_LAYOUTS, INSTANCE_DATA_LAYOUT, mesh_primitive, orient_depth_compare},
    vertex_program::VertexPipelines,
};

#[derive(Clone, Copy)]
pub(super) enum DepthPass {
    Auxiliary {
        renderables: CaptureRenderables,
        reverse_winding: bool,
        reverse_depth: bool,
        lod_fade: bool,
    },
    Particles,
    Material {
        transmission: bool,
        scene_opaque: bool,
        explicit: bool,
        cull_override: Option<MeshCullMode>,
        renderables: CaptureRenderables,
        multisampling: bool,
        reverse_winding: bool,
        reverse_depth: bool,
        lod_fade: bool,
    },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct DepthVariant {
    wide_points: bool,
    wide_lines: bool,
    topology: MeshTopology,
    coverage: MeshDepthCoverage,
    cull: MeshCullMode,
    mode: Deformation,
    particle: bool,
    multisampling: bool,
    reverse_winding: bool,
    reverse_depth: bool,
    lod_fade: bool,
    conservative: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct DepthPipelineKey {
    variant: DepthVariant,
    program: Option<usize>,
}

pub(super) struct DepthPipelines {
    camera: wgpu::BindGroupLayout,
    point_extents: VecDeque<([u32; 2], wgpu::Buffer)>,
    layouts: [wgpu::PipelineLayout; 8],
    wide_pipelines: [Option<VertexPipelines<DepthVariant>>; 4],
    pipelines: VertexPipelines<DepthVariant>,
    bias: wgpu::DepthBiasState,
    format: wgpu::TextureFormat,
}

impl DepthPipelines {
    pub fn new(
        device: &wgpu::Device,
        materials: &wgpu::BindGroupLayout,
        deformations: &DeformationLayouts,
        bias: wgpu::DepthBiasState,
    ) -> Self {
        Self::with_format(device, materials, deformations, bias, DEPTH_FORMAT)
    }

    pub fn with_format(
        device: &wgpu::Device,
        materials: &wgpu::BindGroupLayout,
        deformations: &DeformationLayouts,
        bias: wgpu::DepthBiasState,
        format: wgpu::TextureFormat,
    ) -> Self {
        let camera = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack 3D depth camera layout"),
            entries: &[0, 1].map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }),
        });
        let layouts = std::array::from_fn(|index| {
            let (mode, wide) = Deformation::at_pipeline(index);
            let extras = deformations.get_for_line(mode, wide);
            let groups = [Some(&camera), Some(materials), None, extras];
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack 3D depth pipeline layout"),
                bind_group_layouts: &groups[..if extras.is_some() { 4 } else { 2 }],
                immediate_size: 0,
            })
        });
        Self {
            camera,
            point_extents: VecDeque::new(),
            layouts,
            pipelines: VertexPipelines::new(device, include_str!("depth.wgsl")),
            wide_pipelines: Default::default(),
            bias,
            format,
        }
    }

    pub fn camera_group(
        &mut self,
        device: &wgpu::Device,
        camera: &wgpu::Buffer,
        size: [u32; 2],
    ) -> wgpu::BindGroup {
        let extent =
            if let Some((_, extent)) = self.point_extents.iter().find(|(old, _)| *old == size) {
                extent.clone()
            } else {
                let [width, height] = size.map(|value| value as f32);
                let mut bytes = Vec::with_capacity(16);
                append_floats(&mut bytes, [width, height, 0.0, 0.0]);
                let extent = buffer(
                    device,
                    "XRack depth point viewport",
                    &bytes,
                    wgpu::BufferUsages::UNIFORM,
                );
                self.point_extents.truncate(15);
                self.point_extents.push_front((size, extent.clone()));
                extent
            };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D depth camera"),
            layout: &self.camera,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: extent.as_entire_binding(),
                },
            ],
        })
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        active: &mut Option<DepthPipelineKey>,
        mesh: &MeshBuffer,
        (range, deformation, reflected, stage): (
            Range<u32>,
            Option<&DeformationBinding>,
            bool,
            DepthPass,
        ),
    ) -> u64 {
        mesh.bind_geometry(pass);
        let mode = deformation.map_or(Deformation::Rigid, |binding| binding.mode);
        if mode.skin() {
            mesh.bind_skin(pass);
        }
        let renderables = match stage {
            DepthPass::Auxiliary { renderables, .. } | DepthPass::Material { renderables, .. } => {
                renderables
            }
            DepthPass::Particles => CaptureRenderables::All,
        };
        let mut draws = 0;
        for part in &mesh.parts {
            if !renderables.includes(part.surface.blend) {
                continue;
            }
            let Some(coverage) = part.depth_coverage else {
                continue;
            };
            let (multisampling, reverse_winding, reverse_depth, cull, lod_fade, conservative) =
                match stage {
                    DepthPass::Material {
                        transmission,
                        scene_opaque,
                        explicit,
                        cull_override,
                        multisampling,
                        reverse_winding,
                        reverse_depth,
                        lod_fade,
                        ..
                    } => {
                        let surface = part.surface;
                        let conservative = scene_opaque
                            && (surface.expanded() || surface.topology == MeshTopology::Triangles)
                            && surface
                                .depth_draw
                                .admits_opaque_prepass(surface.blend, surface.transmission);
                        if !(conservative
                            || explicit
                                && surface.depth_draw == MeshDepthDraw::OpaquePrepass
                                && surface.transmission == transmission)
                        {
                            continue;
                        }
                        (
                            multisampling,
                            reverse_winding,
                            reverse_depth,
                            cull_override.unwrap_or(part.surface.cull),
                            lod_fade,
                            conservative,
                        )
                    }
                    DepthPass::Auxiliary {
                        reverse_winding,
                        reverse_depth,
                        lod_fade,
                        ..
                    } => (
                        false,
                        reverse_winding,
                        reverse_depth,
                        part.surface.cull,
                        lod_fade,
                        false,
                    ),
                    DepthPass::Particles => (false, false, false, part.surface.cull, false, false),
                };
            let variant = DepthVariant {
                wide_points: part.surface.wide_points,
                wide_lines: part.surface.wide_lines,
                topology: part.surface.topology,
                coverage,
                cull,
                mode,
                particle: matches!(stage, DepthPass::Particles),
                multisampling,
                reverse_winding: reverse_winding ^ reflected,
                reverse_depth,
                lod_fade,
                conservative,
            };
            let program = part.material.vertex.as_ref();
            let key = DepthPipelineKey {
                variant,
                program: program.map(|source| Arc::as_ptr(source) as *const () as usize),
            };
            if *active != Some(key) {
                pass.set_pipeline(self.get(device, variant, program));
                *active = Some(key);
            }
            mesh.bind_deformation(pass, deformation, part.surface.wide_lines);
            pass.set_bind_group(1, &part.material.base, &[]);
            mesh.draw_range(pass, part.vertices.clone(), range.clone());
            draws += 1;
        }
        draws
    }

    fn get(
        &mut self,
        device: &wgpu::Device,
        variant: DepthVariant,
        program: Option<&Arc<str>>,
    ) -> &wgpu::RenderPipeline {
        let DepthVariant {
            wide_points,
            wide_lines,
            topology,
            coverage,
            cull,
            mode,
            particle,
            multisampling,
            reverse_winding,
            reverse_depth,
            lod_fade,
            conservative,
        } = variant;
        let mask = coverage.samples_alpha();
        let fragment = mask || lod_fade;
        let pipelines = if wide_lines {
            self.wide_pipelines[mode as usize].get_or_insert_with(|| {
                VertexPipelines::new(
                    device,
                    super::primitive_raster::line_shader(include_str!("depth.wgsl"), mode),
                )
            })
        } else {
            &mut self.pipelines
        };
        pipelines.get(device, program, variant, |module| {
            let skinned_buffers = [
                GEOMETRY_LAYOUTS[0].clone(),
                GEOMETRY_LAYOUTS[1].clone(),
                None,
                SKIN_LAYOUT.filter(|_| mode.skin()),
                INSTANCE_DATA_LAYOUT,
            ];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack 3D depth-only mesh pipeline"),
                layout: Some(&self.layouts[mode.pipeline_index(wide_lines)]),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(match (fragment, mode, particle) {
                        (false, _, true) => "vertex_particle_opaque",
                        (true, _, true) => "vertex_particle_mask",
                        (false, Deformation::Rigid, false) => "vertex_opaque",
                        (true, Deformation::Rigid, false) => "vertex_mask",
                        (false, Deformation::Skin, false) => "vertex_skin_opaque",
                        (true, Deformation::Skin, false) => "vertex_skin_mask",
                        (false, Deformation::Morph, false) => "vertex_morph_opaque",
                        (true, Deformation::Morph, false) => "vertex_morph_mask",
                        (false, Deformation::SkinMorph, false) => "vertex_skin_morph_opaque",
                        (true, Deformation::SkinMorph, false) => "vertex_skin_morph_mask",
                    }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("WIDE_POINTS", f64::from(wide_points))],
                        ..Default::default()
                    },
                    buffers: &skinned_buffers,
                },
                fragment: fragment.then_some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fragment_mask"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("DEPTH_TEST_MATERIAL", f64::from(mask)),
                            ("DEPTH_LOD_FADE", f64::from(lod_fade)),
                            (
                                "DEPTH_ALPHA",
                                match coverage {
                                    MeshDepthCoverage::Opaque | MeshDepthCoverage::Mask => 0.0,
                                    MeshDepthCoverage::NonzeroAlpha => 1.0,
                                    MeshDepthCoverage::OpaqueAlpha => 2.0,
                                },
                            ),
                        ],
                        ..Default::default()
                    },
                    targets: &[],
                }),
                primitive: mesh_primitive(
                    topology,
                    wide_points || wide_lines,
                    Default::default(),
                    reverse_winding,
                    cull,
                ),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: self.format,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(orient_depth_compare(
                        wgpu::CompareFunction::Less,
                        reverse_depth,
                    )),
                    stencil: Default::default(),
                    // Seed depth just behind the surface, so the color pass can
                    // keep its strict comparison and still decide equal-depth ties.
                    bias: if !wide_points && !wide_lines && topology != MeshTopology::Triangles {
                        Default::default()
                    } else if conservative {
                        wgpu::DepthBiasState {
                            constant: if reverse_depth { -2 } else { 2 },
                            ..Default::default()
                        }
                    } else {
                        self.bias
                    },
                }),
                multisample: wgpu::MultisampleState {
                    count: if multisampling { 4 } else { 1 },
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        })
    }
}
