use std::collections::HashMap;

use crate::scene3d::{
    COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, INSTANCE_STRIDE,
    material::{CLEARCOAT_SETTINGS_BINDING, CLEARCOAT_TEXTURES, MATERIAL_TEXTURES},
};
use xrack_runtime::scene3d::MeshVertex;

use super::deform::{Deformation, DeformationLayouts, SKIN_LAYOUT, shader_source};
use super::screen_lighting::surface_targets;
use super::transparency::{ACCUMULATION, REVEALAGE};
use super::volumetric_fog::FogMode;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VolumeMode {
    Off,
    Volume,
    Dispersion,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Surface {
    pub blend: bool,
    pub transmission: bool,
    pub volume: VolumeMode,
    pub double_sided: bool,
    pub normal_mapped: bool,
    pub clearcoat: bool,
}

impl Surface {
    const fn index(self) -> usize {
        4 * self.normal_mapped as usize + 2 * self.blend as usize + self.double_sided as usize
    }
}

#[derive(Clone, Copy)]
pub(super) enum ColorPass {
    Opaque,
    Transmission,
    Weighted,
}

#[derive(Clone, Copy)]
pub(super) struct ColorOutput {
    pub hdr: bool,
    pub multisampling: bool,
    pub pass: ColorPass,
    pub reverse_winding: bool,
    pub screen_reflections: bool,
    pub screen_indirect_light: bool,
    pub lightmapped: bool,
    pub fog: FogMode,
}

impl ColorOutput {
    fn weighted(self) -> bool {
        matches!(self.pass, ColorPass::Weighted)
    }

    fn screen_data(self) -> bool {
        self.screen_reflections || self.screen_indirect_light
    }

    pub(super) fn format(self) -> wgpu::TextureFormat {
        if self.hdr { HDR_FORMAT } else { COLOR_FORMAT }
    }

    pub(super) fn samples(self) -> u32 {
        if self.multisampling { 4 } else { 1 }
    }

    fn multisample_state(self) -> wgpu::MultisampleState {
        wgpu::MultisampleState {
            count: self.samples(),
            ..Default::default()
        }
    }
}

pub(super) const GEOMETRY_LAYOUTS: [Option<wgpu::VertexBufferLayout<'static>>; 2] = [
    Some(wgpu::VertexBufferLayout {
        array_stride: MeshVertex::STRIDE as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4, 9 => Float32x2],
    }),
    Some(wgpu::VertexBufferLayout {
        array_stride: INSTANCE_STRIDE,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Float32],
    }),
];

pub(super) struct Pipelines {
    pub globals: wgpu::BindGroupLayout,
    pub materials: [wgpu::BindGroupLayout; 2],
    pub environment: wgpu::BindGroupLayout,
    pub deformations: DeformationLayouts,
    layouts: [[wgpu::PipelineLayout; 4]; 2],
    shader: wgpu::ShaderModule,
    variants: [[Option<wgpu::RenderPipeline>; 64]; 24],
    weighted: [[Option<wgpu::RenderPipeline>; 32]; 24],
    skybox: [Option<wgpu::RenderPipeline>; 12 * FogMode::COUNT],
    specialized: HashMap<(usize, usize, bool, bool, bool, bool, FogMode), wgpu::RenderPipeline>,
}

impl Pipelines {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack 3D globals layout"),
            entries: &[
                uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                uniform(1, wgpu::ShaderStages::FRAGMENT),
                uniform(2, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let materials = [false, true].map(|coated| {
            let mut bindings = vec![uniform(0, wgpu::ShaderStages::FRAGMENT)];
            for index in 0..MATERIAL_TEXTURES + u32::from(coated) * CLEARCOAT_TEXTURES {
                bindings.extend([
                    wgpu::BindGroupLayoutEntry {
                        binding: 1 + 2 * index,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2 + 2 * index,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ]);
            }
            if coated {
                bindings.push(uniform(
                    CLEARCOAT_SETTINGS_BINDING,
                    wgpu::ShaderStages::FRAGMENT,
                ));
            }
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(if coated {
                    "XRack 3D clearcoat layout"
                } else {
                    "XRack 3D materials layout"
                }),
                entries: &bindings,
            })
        });
        let environment = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack environment layout"),
            entries: &[
                uniform(0, wgpu::ShaderStages::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let deformations = DeformationLayouts::new(device);
        let layouts = std::array::from_fn(|coat| {
            std::array::from_fn(|index| {
                let extra = deformations.get(Deformation::ALL[index]);
                let bind_groups = [
                    Some(&globals),
                    Some(&materials[coat]),
                    Some(&environment),
                    extra,
                ];
                device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("XRack 3D pipeline layout"),
                    bind_group_layouts: &bind_groups[..if extra.is_some() { 4 } else { 3 }],
                    immediate_size: 0,
                })
            })
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack 3D mesh shader"),
            source: shader_source(&format!(
                "{}\n{}\n{}\n{}\n{}",
                include_str!("screen_packing.wgsl"),
                include_str!("volumetric_fog.wgsl"),
                include_str!("volumetric_fog_profile.wgsl"),
                include_str!("volumetric_fog_noise.wgsl"),
                include_str!("scene.wgsl")
            )),
        });
        Self {
            globals,
            materials,
            environment,
            deformations,
            layouts,
            shader,
            variants: [const { [const { None }; 64] }; 24],
            weighted: [const { [const { None }; 32] }; 24],
            skybox: [const { None }; 12 * FogMode::COUNT],
            specialized: HashMap::new(),
        }
    }

    pub(super) fn get(
        &mut self,
        device: &wgpu::Device,
        surface: Surface,
        output: ColorOutput,
        deformation: Deformation,
    ) -> &wgpu::RenderPipeline {
        let coated = usize::from(surface.clearcoat);
        let mode = usize::from(output.multisampling)
            + 2 * coated
            + 4 * surface.volume as usize
            + 12 * usize::from(output.reverse_winding);
        let index = surface.index() + 8 * usize::from(output.hdr) + 16 * deformation as usize;
        let create = || {
            let mut buffers = [
                GEOMETRY_LAYOUTS[0].clone(),
                GEOMETRY_LAYOUTS[1].clone(),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![10 => Snorm16x4],
                }),
                SKIN_LAYOUT,
            ];
            if !surface.normal_mapped {
                buffers[2] = None;
            }
            if output.lightmapped {
                buffers[3] = Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![15 => Float32x2],
                });
            }
            let ordinary = [Some(wgpu::ColorTargetState {
                format: output.format(),
                blend: surface
                    .blend
                    .then_some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })];
            let reflection_targets =
                surface_targets(output.multisampling, output.screen_indirect_light);
            let volume = f64::from(u8::from(surface.volume != VolumeMode::Off));
            let dispersion = f64::from(u8::from(surface.volume == VolumeMode::Dispersion));
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if surface.blend {
                    "XRack 3D blended meshes"
                } else {
                    "XRack 3D depth-writing meshes"
                }),
                layout: Some(&self.layouts[coated][deformation as usize]),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some(
                        match (deformation, surface.normal_mapped, output.lightmapped) {
                            (Deformation::Rigid, false, true) => "vertex_lightmapped",
                            (Deformation::Rigid, true, true) => "vertex_lightmapped_tangent",
                            (Deformation::Rigid, false, false) => "vertex_main",
                            (Deformation::Rigid, true, _) => "vertex_tangent",
                            (Deformation::Skin, false, _) => "vertex_skin",
                            (Deformation::Skin, true, _) => "vertex_skin_tangent",
                            (Deformation::Morph, false, _) => "vertex_morph",
                            (Deformation::Morph, true, _) => "vertex_morph_tangent",
                            (Deformation::SkinMorph, false, _) => "vertex_skin_morph",
                            (Deformation::SkinMorph, true, _) => "vertex_skin_morph_tangent",
                        },
                    ),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("VOLUME_MATERIAL", volume)],
                        ..Default::default()
                    },
                    buffers: &buffers[..if deformation.skin() || output.lightmapped {
                        4
                    } else {
                        2 + usize::from(surface.normal_mapped)
                    }],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(
                        match (
                            output.screen_indirect_light,
                            output.screen_reflections,
                            output.weighted(),
                            surface.clearcoat,
                        ) {
                            (true, _, _, false) => "fragment_indirect",
                            (true, _, _, true) => "fragment_indirect_coat",
                            (false, true, _, false) => "fragment_screen",
                            (false, true, _, true) => "fragment_screen_coat",
                            (false, false, false, false) => "fragment_main",
                            (false, false, false, true) => "fragment_coated",
                            (false, false, true, false) => "fragment_weighted",
                            (false, false, true, true) => "fragment_weighted_coated",
                        },
                    ),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("VOLUME_MATERIAL", volume),
                            ("DISPERSION_MATERIAL", dispersion),
                            ("BAKED_LIGHTMAP", f64::from(u8::from(output.lightmapped))),
                            (
                                "VOLUMETRIC_FOG",
                                f64::from(u8::from(output.fog != FogMode::Off)),
                            ),
                            ("FOG_SCATTERING", f64::from(u8::from(output.fog.lit()))),
                            (
                                "FOG_HEIGHT_PROFILE",
                                f64::from(u8::from(output.fog.profiled())),
                            ),
                            ("FOG_DENSITY_NOISE", f64::from(u8::from(output.fog.noisy()))),
                            (
                                "SCREEN_INDIRECT_LIGHT",
                                f64::from(u8::from(output.screen_indirect_light)),
                            ),
                            (
                                "PACK_REFLECTION_ATTRIBUTES",
                                f64::from(u8::from(output.multisampling)),
                            ),
                            (
                                "SCREEN_REFLECTIONS",
                                if output.screen_reflections { 1.0 } else { 0.0 },
                            ),
                        ],
                        ..Default::default()
                    },
                    targets: if output.screen_data() {
                        &reflection_targets
                    } else if output.weighted() {
                        &WEIGHTED_TARGETS
                    } else {
                        &ordinary
                    },
                }),
                primitive: wgpu::PrimitiveState {
                    front_face: if output.reverse_winding {
                        wgpu::FrontFace::Cw
                    } else {
                        wgpu::FrontFace::Ccw
                    },
                    cull_mode: (!surface.double_sided).then_some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(!surface.blend),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: output.multisample_state(),
                multiview_mask: None,
                cache: None,
            })
        };
        if output.screen_data() || output.lightmapped || output.fog != FogMode::Off {
            self.specialized
                .entry((
                    mode,
                    index,
                    output.screen_reflections,
                    output.screen_indirect_light,
                    output.lightmapped,
                    output.weighted(),
                    output.fog,
                ))
                .or_insert_with(create)
        } else if output.weighted() {
            self.weighted[mode][surface.index() + 8 * deformation as usize]
                .get_or_insert_with(create)
        } else {
            self.variants[mode][index].get_or_insert_with(create)
        }
    }

    pub(super) fn skybox(
        &mut self,
        device: &wgpu::Device,
        output: ColorOutput,
    ) -> &wgpu::RenderPipeline {
        let stage = if output.screen_indirect_light {
            2
        } else {
            usize::from(output.screen_reflections)
        };
        let index = 2 * (stage + 3 * output.fog as usize)
            + usize::from(output.multisampling)
            + 6 * FogMode::COUNT * usize::from(output.hdr);
        self.skybox[index].get_or_insert_with(|| {
            let targets = surface_targets(output.multisampling, output.screen_indirect_light);
            let ordinary = [Some(wgpu::ColorTargetState {
                format: output.format(),
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })];
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack skybox pipeline layout"),
                bind_group_layouts: &[Some(&self.globals), None, Some(&self.environment)],
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack skybox"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some("sky_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(if output.screen_indirect_light {
                        "sky_with_indirect"
                    } else if output.screen_reflections {
                        "sky_with_surfaces"
                    } else {
                        "sky_fragment"
                    }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            (
                                "VOLUMETRIC_FOG",
                                f64::from(u8::from(output.fog != FogMode::Off)),
                            ),
                            ("FOG_SCATTERING", f64::from(u8::from(output.fog.lit()))),
                            (
                                "FOG_HEIGHT_PROFILE",
                                f64::from(u8::from(output.fog.profiled())),
                            ),
                            ("FOG_DENSITY_NOISE", f64::from(u8::from(output.fog.noisy()))),
                        ],
                        ..Default::default()
                    },
                    targets: if output.screen_data() {
                        &targets
                    } else {
                        &ordinary
                    },
                }),
                primitive: Default::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(if output.fog != FogMode::Off {
                        wgpu::CompareFunction::Equal
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: output.multisample_state(),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}

const ADDITIVE: wgpu::BlendComponent = wgpu::BlendComponent {
    src_factor: wgpu::BlendFactor::One,
    dst_factor: wgpu::BlendFactor::One,
    operation: wgpu::BlendOperation::Add,
};

pub(super) const WEIGHTED_TARGETS: [Option<wgpu::ColorTargetState>; 2] = [
    Some(wgpu::ColorTargetState {
        format: ACCUMULATION,
        blend: Some(wgpu::BlendState {
            color: ADDITIVE,
            alpha: ADDITIVE,
        }),
        write_mask: wgpu::ColorWrites::ALL,
    }),
    Some(wgpu::ColorTargetState {
        format: REVEALAGE,
        blend: Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::OneMinusSrc,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        }),
        write_mask: wgpu::ColorWrites::RED,
    }),
];
