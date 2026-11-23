use std::ops::Range;

use super::{
    DEPTH_FORMAT, MeshBuffer,
    deform::{Deformation, DeformationBinding, DeformationLayouts, SKIN_LAYOUT, shader_source},
    pipeline::GEOMETRY_LAYOUTS,
};

pub(super) type DepthVariant = (bool, bool, Deformation);

pub(super) struct DepthPipelines {
    camera: wgpu::BindGroupLayout,
    layouts: [wgpu::PipelineLayout; 4],
    shader: wgpu::ShaderModule,
    variants: [Option<wgpu::RenderPipeline>; 16],
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
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D depth shader"),
            source: shader_source(include_str!("depth.wgsl")),
        });
        Self {
            camera,
            layouts,
            shader,
            variants: Default::default(),
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
        active: &mut Option<DepthVariant>,
        mesh: &MeshBuffer,
        (range, deformation): (Range<u32>, Option<&DeformationBinding>),
    ) -> u64 {
        pass.set_vertex_buffer(0, mesh.vertices.slice(..));
        let mode = deformation.map_or(Deformation::Rigid, |binding| binding.mode);
        if mode.skin() {
            mesh.bind_skin(pass);
        }
        let mut draws = 0;
        for part in mesh
            .parts
            .iter()
            .filter(|part| !part.surface.blend && !part.surface.transmission)
        {
            let variant = (part.mask, part.surface.double_sided, mode);
            if *active != Some(variant) {
                pass.set_pipeline(self.get(device, variant));
                *active = Some(variant);
            }
            if let Some(binding) = deformation {
                pass.set_bind_group(3, &binding.group, &[]);
            }
            pass.set_bind_group(1, &part.material.base, &[]);
            pass.draw(part.vertices.clone(), range.clone());
            draws += 1;
        }
        draws
    }

    fn get(
        &mut self,
        device: &wgpu::Device,
        (mask, double_sided, mode): DepthVariant,
    ) -> &wgpu::RenderPipeline {
        self.variants[2 * usize::from(mask) + usize::from(double_sided) + 4 * mode as usize]
            .get_or_insert_with(|| {
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
                        module: &self.shader,
                        entry_point: Some(match (mask, mode) {
                            (false, Deformation::Rigid) => "vertex_opaque",
                            (true, Deformation::Rigid) => "vertex_mask",
                            (false, Deformation::Skin) => "vertex_skin_opaque",
                            (true, Deformation::Skin) => "vertex_skin_mask",
                            (false, Deformation::Morph) => "vertex_morph_opaque",
                            (true, Deformation::Morph) => "vertex_morph_mask",
                            (false, Deformation::SkinMorph) => "vertex_skin_morph_opaque",
                            (true, Deformation::SkinMorph) => "vertex_skin_morph_mask",
                        }),
                        compilation_options: Default::default(),
                        buffers: if mode.skin() {
                            &skinned_buffers
                        } else {
                            &GEOMETRY_LAYOUTS
                        },
                    },
                    fragment: mask.then_some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some("fragment_mask"),
                        compilation_options: Default::default(),
                        targets: &[],
                    }),
                    primitive: wgpu::PrimitiveState {
                        cull_mode: (!double_sided).then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
                        bias: self.bias,
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                })
            })
    }
}
