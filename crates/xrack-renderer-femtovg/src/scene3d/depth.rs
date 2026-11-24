use std::{ops::Range, sync::Arc};

use xrack_runtime::{
    scene_effect::CaptureRenderables,
    scene3d::{MeshCullMode, MeshDepthCoverage, MeshDepthDraw},
};

use super::{
    DEPTH_FORMAT, MeshBuffer,
    deform::{Deformation, DeformationBinding, DeformationLayouts, SKIN_LAYOUT},
    pipeline::{GEOMETRY_LAYOUTS, culled_face},
    vertex_program::VertexPipelines,
};

#[derive(Clone, Copy)]
pub(super) enum DepthPass {
    Auxiliary(CaptureRenderables),
    Particles,
    Material {
        transmission: bool,
        cull_override: Option<MeshCullMode>,
        renderables: CaptureRenderables,
        multisampling: bool,
        reverse_winding: bool,
    },
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct DepthVariant {
    coverage: MeshDepthCoverage,
    cull: MeshCullMode,
    mode: Deformation,
    particle: bool,
    multisampling: bool,
    reverse_winding: bool,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct DepthPipelineKey {
    variant: DepthVariant,
    program: Option<usize>,
}

pub(super) struct DepthPipelines {
    camera: wgpu::BindGroupLayout,
    layouts: [wgpu::PipelineLayout; 4],
    pipelines: VertexPipelines<DepthVariant>,
    bias: wgpu::DepthBiasState,
}

impl DepthPipelines {
    pub fn new(
        device: &wgpu::Device,
        materials: &wgpu::BindGroupLayout,
        deformations: &DeformationLayouts,
        bias: wgpu::DepthBiasState,
    ) -> Self {
        let camera = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack 3D depth camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let layouts = std::array::from_fn(|index| {
            let extras = deformations.get(Deformation::ALL[index]);
            let groups = [Some(&camera), Some(materials), None, extras];
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack 3D depth pipeline layout"),
                bind_group_layouts: &groups[..if extras.is_some() { 4 } else { 2 }],
                immediate_size: 0,
            })
        });
        Self {
            camera,
            layouts,
            pipelines: VertexPipelines::new(device, include_str!("depth.wgsl")),
            bias,
        }
    }

    pub fn camera_group(&self, device: &wgpu::Device, camera: &wgpu::Buffer) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D depth camera"),
            layout: &self.camera,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        })
    }

    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        active: &mut Option<DepthPipelineKey>,
        mesh: &MeshBuffer,
        (range, deformation, stage): (Range<u32>, Option<&DeformationBinding>, DepthPass),
    ) -> u64 {
        mesh.bind_geometry(pass);
        let mode = deformation.map_or(Deformation::Rigid, |binding| binding.mode);
        if mode.skin() {
            mesh.bind_skin(pass);
        }
        let renderables = match stage {
            DepthPass::Auxiliary(renderables) | DepthPass::Material { renderables, .. } => {
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
            let (multisampling, reverse_winding, cull) = match stage {
                DepthPass::Material {
                    transmission,
                    cull_override,
                    multisampling,
                    reverse_winding,
                    ..
                } => {
                    if part.surface.depth_draw != MeshDepthDraw::OpaquePrepass
                        || part.surface.transmission != transmission
                    {
                        continue;
                    }
                    (
                        multisampling,
                        reverse_winding,
                        cull_override.unwrap_or(part.surface.cull),
                    )
                }
                _ => (false, false, part.surface.cull),
            };
            let variant = DepthVariant {
                coverage,
                cull,
                mode,
                particle: matches!(stage, DepthPass::Particles),
                multisampling,
                reverse_winding,
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
            if let Some(binding) = deformation {
                pass.set_bind_group(3, &binding.group, &[]);
            }
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
            coverage,
            cull,
            mode,
            particle,
            multisampling,
            reverse_winding,
        } = variant;
        let mask = coverage.samples_alpha();
        self.pipelines.get(device, program, variant, |module| {
            let skinned_buffers = [
                GEOMETRY_LAYOUTS[0].clone(),
                GEOMETRY_LAYOUTS[1].clone(),
                None,
                SKIN_LAYOUT,
            ];
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack 3D depth-only mesh pipeline"),
                layout: Some(&self.layouts[mode as usize]),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(match (mask, mode, particle) {
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
                    compilation_options: Default::default(),
                    buffers: if mode.skin() {
                        &skinned_buffers
                    } else {
                        &GEOMETRY_LAYOUTS
                    },
                },
                fragment: mask.then_some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fragment_mask"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[(
                            "DEPTH_ALPHA",
                            match coverage {
                                MeshDepthCoverage::Opaque | MeshDepthCoverage::Mask => 0.0,
                                MeshDepthCoverage::NonzeroAlpha => 1.0,
                                MeshDepthCoverage::OpaqueAlpha => 2.0,
                            },
                        )],
                        ..Default::default()
                    },
                    targets: &[],
                }),
                primitive: wgpu::PrimitiveState {
                    front_face: if reverse_winding {
                        wgpu::FrontFace::Cw
                    } else {
                        wgpu::FrontFace::Ccw
                    },
                    cull_mode: culled_face(cull),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: self.bias,
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
