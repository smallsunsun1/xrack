use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use crate::scene3d::{
    COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, INSTANCE_STRIDE,
    color_capture::attachment_format,
    material::{
        CLEARCOAT_SETTINGS_BINDING, CLEARCOAT_TEXTURES, FIRST_SHADER_TEXTURE_BINDING,
        MATERIAL_TEXTURES, SHADER_CAPTURE_BINDING, shader_texture_slots,
    },
};
use xrack_runtime::{
    material_shader::{INTERFACE, MaterialHooks, VALIDATION_TEXTURES, VERTEX_APPLICATION},
    scene_effect::{
        CAPTURE_DEPTH_READ, CAPTURE_POSITION, CaptureAttachment, CaptureBlend,
        CaptureBlendComponent, CaptureBlendFactor, CaptureBlendOp, CaptureDepthCompare,
        CapturePipelineState, CaptureRenderables, CaptureStencilOp,
    },
    scene3d::{MeshBlendMode, MeshCullMode, MeshDepthDraw, MeshVertex},
};

use super::deform::{Deformation, DeformationLayouts, SKIN_LAYOUT, shader_source};
use super::screen_lighting::surface_targets;
use super::transparency::{ACCUMULATION, REVEALAGE};
use super::volumetric_fog::FogMode;
use super::wireframe::{self, RasterMode};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VolumeMode {
    Off,
    Volume,
    Dispersion,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Surface {
    pub blend: bool,
    pub blend_mode: MeshBlendMode,
    pub depth_draw: MeshDepthDraw,
    pub channel_remapping: bool,
    pub vertex_colors: bool,
    pub vertex_masks: bool,
    pub custom_base_fresnel: bool,
    pub custom_coat_fresnel: bool,
    pub transmission: bool,
    pub volume: VolumeMode,
    pub cull: MeshCullMode,
    pub vertex_tangents: bool,
    pub anisotropy: bool,
    pub clearcoat: bool,
}

impl Surface {
    const fn index(self) -> usize {
        4 * self.vertex_tangents as usize
            + 2 * self.blend as usize
            + matches!(self.cull, MeshCullMode::None) as usize
    }
}

pub(super) fn culled_face(mode: MeshCullMode) -> Option<wgpu::Face> {
    match mode {
        MeshCullMode::Back => Some(wgpu::Face::Back),
        MeshCullMode::Front => Some(wgpu::Face::Front),
        MeshCullMode::None => None,
    }
}

fn blend_state(mode: MeshBlendMode) -> wgpu::BlendState {
    use wgpu::BlendFactor::{Dst, One, OneMinusSrc, OneMinusSrcAlpha, Zero};
    let (src_factor, dst_factor) = match mode {
        MeshBlendMode::SourceOver => return wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING,
        MeshBlendMode::Screen => (One, OneMinusSrc),
        MeshBlendMode::Multiply => (Dst, OneMinusSrcAlpha),
    };
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor,
            dst_factor,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: if mode == MeshBlendMode::Multiply {
            wgpu::BlendComponent {
                src_factor: Zero,
                dst_factor: One,
                operation: wgpu::BlendOperation::Add,
            }
        } else {
            wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha
        },
    }
}

pub(super) fn override_blend(mode: CaptureBlend) -> Option<wgpu::BlendState> {
    match mode {
        CaptureBlend::Replace => None,
        CaptureBlend::SourceOver => Some(blend_state(MeshBlendMode::SourceOver)),
        CaptureBlend::Screen => Some(blend_state(MeshBlendMode::Screen)),
        CaptureBlend::Multiply => Some(blend_state(MeshBlendMode::Multiply)),
        CaptureBlend::Additive => Some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING.alpha,
        }),
        CaptureBlend::Custom { color, alpha } => Some(wgpu::BlendState {
            color: blend_component(color, false),
            alpha: blend_component(alpha, true),
        }),
    }
}

fn blend_component(component: CaptureBlendComponent, alpha: bool) -> wgpu::BlendComponent {
    let factor = |value| match value {
        CaptureBlendFactor::Zero => wgpu::BlendFactor::Zero,
        CaptureBlendFactor::One => wgpu::BlendFactor::One,
        CaptureBlendFactor::Src if alpha => wgpu::BlendFactor::SrcAlpha,
        CaptureBlendFactor::Src => wgpu::BlendFactor::Src,
        CaptureBlendFactor::OneMinusSrc if alpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        CaptureBlendFactor::OneMinusSrc => wgpu::BlendFactor::OneMinusSrc,
        CaptureBlendFactor::SrcAlpha => wgpu::BlendFactor::SrcAlpha,
        CaptureBlendFactor::OneMinusSrcAlpha => wgpu::BlendFactor::OneMinusSrcAlpha,
        CaptureBlendFactor::Dst if alpha => wgpu::BlendFactor::DstAlpha,
        CaptureBlendFactor::Dst => wgpu::BlendFactor::Dst,
        CaptureBlendFactor::OneMinusDst if alpha => wgpu::BlendFactor::OneMinusDstAlpha,
        CaptureBlendFactor::OneMinusDst => wgpu::BlendFactor::OneMinusDst,
        CaptureBlendFactor::DstAlpha => wgpu::BlendFactor::DstAlpha,
        CaptureBlendFactor::OneMinusDstAlpha => wgpu::BlendFactor::OneMinusDstAlpha,
        CaptureBlendFactor::SrcAlphaSaturated if alpha => wgpu::BlendFactor::One,
        CaptureBlendFactor::SrcAlphaSaturated => wgpu::BlendFactor::SrcAlphaSaturated,
        CaptureBlendFactor::Constant => wgpu::BlendFactor::Constant,
        CaptureBlendFactor::OneMinusConstant => wgpu::BlendFactor::OneMinusConstant,
    };
    wgpu::BlendComponent {
        src_factor: factor(component.source),
        dst_factor: factor(component.destination),
        operation: match component.operation {
            CaptureBlendOp::Add => wgpu::BlendOperation::Add,
            CaptureBlendOp::Subtract => wgpu::BlendOperation::Subtract,
            CaptureBlendOp::ReverseSubtract => wgpu::BlendOperation::ReverseSubtract,
            CaptureBlendOp::Min => wgpu::BlendOperation::Min,
            CaptureBlendOp::Max => wgpu::BlendOperation::Max,
        },
    }
}

pub(super) fn fixed_state(mut state: CapturePipelineState) -> CapturePipelineState {
    // The constant and reference are set on the pass, not the pipeline.
    state.blend_constant = None;
    if let Some(stencil) = &mut state.stencil {
        stencil.reference = 0;
    }
    state
}

pub(super) fn depth_compare(
    state: CapturePipelineState,
    default: wgpu::CompareFunction,
    reverse_depth: bool,
) -> wgpu::CompareFunction {
    orient_depth_compare(
        state.depth_compare.map_or(default, compare_function),
        reverse_depth,
    )
}

pub(super) fn orient_depth_compare(
    compare: wgpu::CompareFunction,
    reverse_depth: bool,
) -> wgpu::CompareFunction {
    use wgpu::CompareFunction::*;
    if reverse_depth {
        match compare {
            Less => Greater,
            LessEqual => GreaterEqual,
            Greater => Less,
            GreaterEqual => LessEqual,
            _ => compare,
        }
    } else {
        compare
    }
}

fn compare_function(value: CaptureDepthCompare) -> wgpu::CompareFunction {
    match value {
        CaptureDepthCompare::Never => wgpu::CompareFunction::Never,
        CaptureDepthCompare::Less => wgpu::CompareFunction::Less,
        CaptureDepthCompare::LessEqual => wgpu::CompareFunction::LessEqual,
        CaptureDepthCompare::Equal => wgpu::CompareFunction::Equal,
        CaptureDepthCompare::NotEqual => wgpu::CompareFunction::NotEqual,
        CaptureDepthCompare::GreaterEqual => wgpu::CompareFunction::GreaterEqual,
        CaptureDepthCompare::Greater => wgpu::CompareFunction::Greater,
        CaptureDepthCompare::Always => wgpu::CompareFunction::Always,
    }
}

pub(super) fn stencil_state(state: CapturePipelineState) -> wgpu::StencilState {
    let Some(stencil) = state.stencil else {
        return wgpu::StencilState::default();
    };
    let operation = |op| match op {
        CaptureStencilOp::Keep => wgpu::StencilOperation::Keep,
        CaptureStencilOp::Zero => wgpu::StencilOperation::Zero,
        CaptureStencilOp::Replace => wgpu::StencilOperation::Replace,
        CaptureStencilOp::Invert => wgpu::StencilOperation::Invert,
        CaptureStencilOp::IncrementClamp => wgpu::StencilOperation::IncrementClamp,
        CaptureStencilOp::DecrementClamp => wgpu::StencilOperation::DecrementClamp,
        CaptureStencilOp::IncrementWrap => wgpu::StencilOperation::IncrementWrap,
        CaptureStencilOp::DecrementWrap => wgpu::StencilOperation::DecrementWrap,
    };
    let face = wgpu::StencilFaceState {
        compare: compare_function(stencil.compare),
        fail_op: operation(stencil.fail),
        depth_fail_op: operation(stencil.depth_fail),
        pass_op: operation(stencil.pass),
    };
    wgpu::StencilState {
        front: face,
        back: face,
        read_mask: u32::from(stencil.read_mask),
        write_mask: u32::from(stencil.write_mask),
    }
}

pub(super) fn color_writes(state: CapturePipelineState) -> wgpu::ColorWrites {
    wgpu::ColorWrites::from_bits_truncate(u32::from(state.color_write_mask))
}

pub(super) fn uses_material_prepass(state: CapturePipelineState) -> bool {
    !state.wireframe && state.depth_compare.is_none() && state.depth_write.is_none()
}

#[derive(Clone, Copy)]
pub(super) enum ColorPass {
    Opaque,
    Transmission,
    Transparent,
    Weighted,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum GeometryOutput {
    Surface,
    Named(u8),
    Data(wgpu::TextureFormat),
    Combined(u8, Option<wgpu::TextureFormat>),
}

pub(super) const NAMED_GEOMETRY_ATTACHMENTS: [CaptureAttachment; 4] = [
    CaptureAttachment::NormalRoughness,
    CaptureAttachment::BaseColorMetalness,
    CaptureAttachment::EmissiveOcclusion,
    CaptureAttachment::Depth,
];

impl GeometryOutput {
    fn has_attachment(self, attachment: CaptureAttachment) -> bool {
        match self {
            Self::Named(mask) | Self::Combined(mask, _) => mask & (1 << attachment.index()) != 0,
            Self::Data(_) => attachment == CaptureAttachment::MaterialData,
            Self::Surface => false,
        }
    }

    pub(super) fn combined(self) -> bool {
        matches!(self, Self::Combined(..))
    }

    fn geometry_targets(self, writes: wgpu::ColorWrites) -> [Option<wgpu::ColorTargetState>; 4] {
        NAMED_GEOMETRY_ATTACHMENTS.map(|attachment| {
            self.has_attachment(attachment)
                .then(|| wgpu::ColorTargetState {
                    format: attachment_format(attachment),
                    blend: None,
                    write_mask: writes,
                })
        })
    }

    pub(super) fn combined_targets(
        self,
        color: wgpu::ColorTargetState,
        writes: wgpu::ColorWrites,
    ) -> Option<([Option<wgpu::ColorTargetState>; 6], usize)> {
        let Self::Combined(_, data_format) = self else {
            return None;
        };
        let [normal, base, emissive, depth] = self.geometry_targets(writes);
        let data = data_format.map(|format| wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: writes,
        });
        let len = NAMED_GEOMETRY_ATTACHMENTS.len() + 1 + usize::from(data.is_some());
        Some(([normal, base, emissive, depth, Some(color), data], len))
    }
}

#[derive(Clone, Copy)]
pub(super) struct ColorOutput {
    pub hdr: bool,
    pub format_override: Option<wgpu::TextureFormat>,
    pub multisampling: bool,
    pub pass: ColorPass,
    pub renderables: CaptureRenderables,
    pub pipeline_state: CapturePipelineState,
    pub stenciled: bool,
    pub raster: RasterMode,
    pub reverse_winding: bool,
    pub reverse_depth: bool,
    pub screen_reflections: bool,
    pub screen_indirect_light: bool,
    pub capture_geometry: Option<GeometryOutput>,
    pub named_geometry: bool,
    pub lightmapped: bool,
    pub model_particles: bool,
    pub fog: FogMode,
}

impl ColorOutput {
    pub(super) fn writes_material_data(self) -> bool {
        self.capture_geometry
            .is_some_and(|geometry| geometry.has_attachment(CaptureAttachment::MaterialData))
    }

    pub(super) fn draws_geometry(self) -> bool {
        self.renderables.includes(self.transparent())
    }

    pub(super) fn transparent(self) -> bool {
        matches!(self.pass, ColorPass::Transparent | ColorPass::Weighted)
    }

    fn weighted(self) -> bool {
        matches!(self.pass, ColorPass::Weighted)
    }

    fn screen_data(self) -> bool {
        self.screen_reflections || self.screen_indirect_light
    }

    pub(super) fn depth_format(self) -> wgpu::TextureFormat {
        if self.stenciled {
            super::capture_stencil::FORMAT
        } else {
            DEPTH_FORMAT
        }
    }

    pub(super) fn format(self) -> wgpu::TextureFormat {
        match self.capture_geometry {
            Some(GeometryOutput::Surface | GeometryOutput::Named(_)) => HDR_FORMAT,
            Some(GeometryOutput::Data(format)) => format,
            _ => self
                .format_override
                .unwrap_or(if self.hdr { HDR_FORMAT } else { COLOR_FORMAT }),
        }
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
        attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Uint32],
    }),
];

// Skin and lightmap streams are mutually exclusive and reuse location 11.
pub(super) const INSTANCE_DATA_LAYOUT: Option<wgpu::VertexBufferLayout<'static>> =
    Some(wgpu::VertexBufferLayout {
        array_stride: 16,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![15 => Float32x4],
    });

pub(super) struct Pipelines {
    pub globals: wgpu::BindGroupLayout,
    pub materials: [wgpu::BindGroupLayout; 2],
    pub environment: wgpu::BindGroupLayout,
    pub shader_materials: [[Option<wgpu::BindGroupLayout>; 2]; 2],
    pub deformations: DeformationLayouts,
    layouts: [[wgpu::PipelineLayout; 4]; 2],
    shader: wgpu::ShaderModule,
    wire_shader: Option<wgpu::ShaderModule>,
    variants: [[Option<wgpu::RenderPipeline>; 64]; 24],
    weighted: [[Option<wgpu::RenderPipeline>; 32]; 24],
    skybox: [Option<wgpu::RenderPipeline>; 48 * FogMode::COUNT],
    skybox_formats: HashMap<(usize, wgpu::TextureFormat), wgpu::RenderPipeline>,
    specialized: HashMap<VariantKey, wgpu::RenderPipeline>,
    custom: CustomMaterials,
}

type VariantKey = (
    usize,
    usize,
    bool,
    bool,
    bool,
    bool,
    bool,
    FogMode,
    MeshBlendMode,
    MeshDepthDraw,
    MeshCullMode,
    (
        bool,
        bool,
        bool,
        bool,
        bool,
        bool,
        Option<GeometryOutput>,
        CapturePipelineState,
        bool,
        RasterMode,
        Option<wgpu::TextureFormat>,
        bool,
    ),
);

#[derive(Default)]
struct CustomMaterials {
    programs: VecDeque<CustomProgram>,
    builds: u64,
}

struct CustomProgram {
    source: Arc<str>,
    slots: usize,
    wireframe: bool,
    module: Option<wgpu::ShaderModule>,
    pipelines: HashMap<VariantKey, wgpu::RenderPipeline>,
}

impl CustomMaterials {
    fn get(
        &mut self,
        device: &wgpu::Device,
        (source, slots, wireframe): (&Arc<str>, usize, bool),
        key: VariantKey,
        create: impl Fn(Option<&wgpu::ShaderModule>) -> wgpu::RenderPipeline,
    ) -> Option<&wgpu::RenderPipeline> {
        const RETAINED_PROGRAMS: usize = 16;
        let index = self.programs.iter().position(|program| {
            program.slots == slots && program.wireframe == wireframe && program.source == *source
        });
        if index != Some(0) {
            let program = index
                .and_then(|index| self.programs.remove(index))
                .unwrap_or_else(|| {
                    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                    let module = mesh_shader(device, source, slots, wireframe);
                    let module = match pollster::block_on(scope.pop()) {
                        Some(error) => {
                            tracing::warn!(%error, "XRack disabled an invalid material shader");
                            None
                        }
                        None => Some(module),
                    };
                    CustomProgram {
                        source: source.clone(),
                        slots,
                        wireframe,
                        module,
                        pipelines: HashMap::new(),
                    }
                });
            self.programs.truncate(RETAINED_PROGRAMS - 1);
            self.programs.push_front(program);
        }
        let program = self.programs.front_mut()?;
        Some(program.pipelines.entry(key).or_insert_with(|| {
            let Some(module) = &program.module else {
                return create(None);
            };
            self.builds += 1;
            let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
            let pipeline = create(Some(module));
            if let Some(error) = pollster::block_on(scope.pop()) {
                tracing::warn!(%error, "XRack fell back to built-in material shading");
                create(None)
            } else {
                pipeline
            }
        }))
    }
}

fn texture_interface(slots: usize, positions: bool) -> String {
    let bindings: String = (0..slots)
        .map(|slot| {
            let binding = FIRST_SHADER_TEXTURE_BINDING + slot as u32;
            format!(
                "@group(1) @binding({binding}) var xr_material_image_{slot}: texture_2d<f32>;\n"
            )
        })
        .collect();
    let sizes: String = (0..slots)
        .map(|slot| {
            format!("case {slot}u: {{ return textureDimensions(xr_material_image_{slot}); }}\n")
        })
        .collect();
    let samples: String = (0..slots)
        .map(|slot| format!("case {slot}u: {{ return xr_material_sample(xr_material_image_{slot}, uv, flags); }}\n"))
        .collect();
    let positions = if positions {
        let cases: String = (0..slots).map(|slot| format!(
            "case {slot}u: {{ return xr_material_world_position(xr_material_image_{slot}, uv, flags, xr_material_inverse_clip[{slot}]); }}\n"
        )).collect();
        format!(
            r#"
@group(1) @binding({SHADER_CAPTURE_BINDING}) var<uniform> xr_material_inverse_clip: array<mat4x4<f32>, {slots}>;
fn material_texture_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition {{
    if !material_has_texture(index) || !all(abs(uv) <= vec2(1e20)) {{
        return xr_invalid_capture_position();
    }}
    let flags = material.shader_texture_flags[index];
    switch index {{
        {cases}
        default: {{ return xr_invalid_capture_position(); }}
    }}
}}"#
        )
    } else {
        "fn material_texture_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition { return xr_invalid_capture_position(); }".to_owned()
    };
    format!(
        r#"
{bindings}
{}
fn material_has_texture(index: u32) -> bool {{
    return index < {slots}u && (material.shader_texture_flags[index] & 1u) != 0u;
}}
fn material_texture_size(index: u32) -> vec2<u32> {{
    if !material_has_texture(index) {{ return vec2(0u); }}
    switch index {{
        {sizes}
        default: {{ return vec2(0u); }}
    }}
}}
fn material_texture(index: u32, uv: vec2<f32>) -> vec4<f32> {{
    if !material_has_texture(index) || !all(abs(uv) <= vec2(1e20)) {{
        return vec4(0.0);
    }}
    let flags = material.shader_texture_flags[index];
    switch index {{
        {samples}
        default: {{ return vec4(0.0); }}
    }}
}}
{positions}"#,
        include_str!("material_sampling.wgsl")
    )
}

fn mesh_shader(
    device: &wgpu::Device,
    custom: &str,
    slots: usize,
    wireframe: bool,
) -> wgpu::ShaderModule {
    let standard = include_str!("scene.wgsl");
    let scene = if wireframe {
        std::borrow::Cow::Owned(wireframe::mesh(standard))
    } else {
        std::borrow::Cow::Borrowed(standard)
    };
    let textures = if slots == 0 {
        VALIDATION_TEXTURES.to_owned()
    } else {
        texture_interface(slots, MaterialHooks::from_source(custom).capture_position)
    };
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack 3D mesh shader"),
        source: shader_source(&format!(
            "{custom}\n{INTERFACE}\n{CAPTURE_POSITION}\n{CAPTURE_DEPTH_READ}\n{textures}\n{VERTEX_APPLICATION}\n{}\n{}\n{}\n{}\n{}",
            include_str!("screen_packing.wgsl"),
            include_str!("volumetric_fog.wgsl"),
            include_str!("volumetric_fog_profile.wgsl"),
            include_str!("volumetric_fog_noise.wgsl"),
            scene
        )),
    })
}

fn material_layout(
    device: &wgpu::Device,
    coated: bool,
    shader_slots: usize,
    positions: bool,
) -> wgpu::BindGroupLayout {
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
    let mut bindings = vec![uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT)];
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
    for index in 0..shader_slots {
        bindings.push(wgpu::BindGroupLayoutEntry {
            binding: FIRST_SHADER_TEXTURE_BINDING + index as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    if positions {
        bindings.push(uniform(
            SHADER_CAPTURE_BINDING,
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
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
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
        let materials = [false, true].map(|coated| material_layout(device, coated, 0, false));
        let shader_materials = [false, true].map(|coated| {
            let slots = shader_texture_slots(device, coated);
            [false, true].map(|positions| {
                (slots > 0).then(|| material_layout(device, coated, slots, positions))
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
        let shader = mesh_shader(device, &MaterialHooks::default().prepare(""), 0, false);
        Self {
            globals,
            materials,
            shader_materials,
            environment,
            deformations,
            layouts,
            shader,
            wire_shader: None,
            variants: [const { [const { None }; 64] }; 24],
            weighted: [const { [const { None }; 32] }; 24],
            skybox: [const { None }; 48 * FogMode::COUNT],
            skybox_formats: HashMap::new(),
            specialized: HashMap::new(),
            custom: CustomMaterials::default(),
        }
    }

    pub(super) fn get(
        &mut self,
        device: &wgpu::Device,
        surface: Surface,
        output: ColorOutput,
        deformation: Deformation,
        custom: Option<(&Arc<str>, usize)>,
    ) -> &wgpu::RenderPipeline {
        let coated = usize::from(surface.clearcoat);
        let mode = usize::from(output.multisampling)
            + 2 * coated
            + 4 * surface.volume as usize
            + 12 * usize::from(output.reverse_winding);
        let index = surface.index() + 8 * usize::from(output.hdr) + 16 * deformation as usize;
        let custom_slots = custom.map_or(0, |(_, slots)| slots);
        let hooks = custom.map_or_else(MaterialHooks::default, |(source, _)| {
            MaterialHooks::from_source(source)
        });
        let writes_material_data = output.writes_material_data();

        let wireframe = output.raster == RasterMode::ShaderLine;
        let shader = if wireframe {
            self.wire_shader.get_or_insert_with(|| {
                mesh_shader(device, &MaterialHooks::default().prepare(""), 0, true)
            })
        } else {
            &self.shader
        };
        let create = |custom: Option<&wgpu::ShaderModule>| {
            let custom_fragment = custom.filter(|_| hooks.uses_fragment(writes_material_data));
            let layout = self.shader_materials[coated][usize::from(hooks.capture_position)]
                .as_ref()
                .filter(|_| custom_slots > 0)
                .map(|materials| {
                    let extra = self.deformations.get(deformation);
                    let groups = [
                        Some(&self.globals),
                        Some(materials),
                        Some(&self.environment),
                        extra,
                    ];
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("XRack 3D programmable material pipeline layout"),
                        bind_group_layouts: &groups[..if extra.is_some() { 4 } else { 3 }],
                        immediate_size: 0,
                    })
                });
            let mut buffers = [
                GEOMETRY_LAYOUTS[0].clone(),
                GEOMETRY_LAYOUTS[1].clone(),
                Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![10 => Snorm16x4],
                }),
                SKIN_LAYOUT.filter(|_| deformation.skin()),
                INSTANCE_DATA_LAYOUT,
            ];
            if !surface.vertex_tangents {
                buffers[2] = None;
            }
            if output.lightmapped {
                buffers[3] = Some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![11 => Float32x2],
                });
            }
            let state = output.pipeline_state;
            let material_prepass = !output.stenciled && uses_material_prepass(state);
            let default_blend = surface.blend.then(|| blend_state(surface.blend_mode));
            let color = wgpu::ColorTargetState {
                format: output.format(),
                blend: state.blend.map_or(default_blend, override_blend),
                write_mask: color_writes(state),
            };
            let geometry_targets = output
                .capture_geometry
                .map(|geometry| geometry.geometry_targets(wgpu::ColorWrites::ALL));
            let combined = output
                .capture_geometry
                .is_some_and(GeometryOutput::combined);
            let combined_targets = output.capture_geometry.and_then(|geometry| {
                geometry.combined_targets(color.clone(), wgpu::ColorWrites::ALL)
            });
            let ordinary = [Some(color)];
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
                layout: Some(
                    layout
                        .as_ref()
                        .unwrap_or(&self.layouts[coated][deformation as usize]),
                ),
                vertex: wgpu::VertexState {
                    module: custom.filter(|_| hooks.vertex).unwrap_or(shader),
                    entry_point: Some(
                        match (deformation, surface.vertex_tangents, output.lightmapped) {
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
                        constants: &[
                            ("VOLUME_MATERIAL", volume),
                            (
                                "ANISOTROPIC_MATERIAL",
                                f64::from(u8::from(surface.anisotropy)),
                            ),
                        ],
                        ..Default::default()
                    },
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: custom_fragment.unwrap_or(shader),
                    entry_point: Some(match output.capture_geometry {
                        Some(GeometryOutput::Named(mask))
                            if mask == 1 << CaptureAttachment::Depth.index() =>
                        {
                            "fragment_capture_depth"
                        }
                        Some(GeometryOutput::Named(_)) => "fragment_capture_geometry",
                        Some(GeometryOutput::Data(_)) => "fragment_capture_data",
                        Some(GeometryOutput::Combined(..)) => {
                            match (writes_material_data, surface.clearcoat) {
                                (false, false) => "fragment_capture_lit",
                                (false, true) => "fragment_capture_lit_coat",
                                (true, false) => "fragment_capture_lit_data",
                                (true, true) => "fragment_capture_lit_data_coat",
                            }
                        }
                        Some(GeometryOutput::Surface) => "fragment_main",
                        None => match (
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
                    }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("VOLUME_MATERIAL", volume),
                            ("DISPERSION_MATERIAL", dispersion),
                            (
                                "CAPTURE_EFFECT_SURFACE",
                                f64::from(output.capture_geometry.is_some() && !combined),
                            ),
                            ("CAPTURE_LIT_SURFACE", f64::from(combined)),
                            (
                                "CAPTURE_MATERIAL_DATA",
                                f64::from(
                                    writes_material_data && hooks.data && custom_fragment.is_some(),
                                ),
                            ),
                            (
                                "CAPTURE_MATERIAL_OCCLUSION",
                                f64::from(output.capture_geometry.is_some_and(|geometry| {
                                    geometry.has_attachment(CaptureAttachment::EmissiveOcclusion)
                                })),
                            ),
                            (
                                "CUSTOM_MATERIAL",
                                f64::from(u8::from(custom_fragment.is_some() && hooks.surface)),
                            ),
                            (
                                "ANISOTROPIC_MATERIAL",
                                f64::from(u8::from(surface.anisotropy)),
                            ),
                            (
                                "REMAP_MATERIAL_CHANNELS",
                                f64::from(u8::from(surface.channel_remapping)),
                            ),
                            (
                                "USE_VERTEX_COLORS",
                                f64::from(u8::from(surface.vertex_colors)),
                            ),
                            (
                                "APPLY_VERTEX_MASKS",
                                f64::from(u8::from(surface.vertex_masks)),
                            ),
                            (
                                "CUSTOM_BASE_FRESNEL",
                                f64::from(u8::from(surface.custom_base_fresnel)),
                            ),
                            (
                                "CUSTOM_COAT_FRESNEL",
                                f64::from(u8::from(surface.custom_coat_fresnel)),
                            ),
                            (
                                "NO_DEPTH_EFFECTS",
                                f64::from(u8::from(surface.depth_draw == MeshDepthDraw::Never)),
                            ),
                            (
                                "LIMIT_BLEND_TINT",
                                f64::from(u8::from(state.blend.map_or(
                                    surface.blend_mode != MeshBlendMode::SourceOver,
                                    |mode| {
                                        matches!(
                                            mode,
                                            CaptureBlend::Multiply | CaptureBlend::Screen
                                        )
                                    },
                                ))),
                            ),
                            ("BAKED_LIGHTMAP", f64::from(u8::from(output.lightmapped))),
                            (
                                "MODEL_PARTICLES",
                                f64::from(u8::from(output.model_particles)),
                            ),
                            (
                                "VOLUMETRIC_FOG",
                                f64::from(u8::from(output.fog != FogMode::Off)),
                            ),
                            (
                                "TANGENT_VERTEX",
                                f64::from(u8::from(surface.vertex_tangents)),
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
                    targets: if let Some((targets, len)) = &combined_targets {
                        &targets[..*len]
                    } else if let Some(targets) = &geometry_targets
                        && matches!(output.capture_geometry, Some(GeometryOutput::Named(_)))
                    {
                        targets
                    } else if output.screen_data() {
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
                    cull_mode: culled_face(state.cull.unwrap_or(surface.cull)),
                    polygon_mode: output.raster.polygon_mode(),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: output.depth_format(),
                    depth_write_enabled: Some(state.depth_write.unwrap_or_else(|| {
                        output.capture_geometry.is_some()
                            || if surface.depth_draw == MeshDepthDraw::OpaquePrepass
                                && !material_prepass
                            {
                                !surface.blend
                            } else {
                                surface.depth_draw.writes_color_depth(surface.blend)
                            }
                    })),
                    depth_compare: Some(depth_compare(
                        state,
                        if output.capture_geometry.is_none()
                            && surface.depth_draw == MeshDepthDraw::OpaquePrepass
                            && material_prepass
                        {
                            wgpu::CompareFunction::LessEqual
                        } else {
                            wgpu::CompareFunction::Less
                        },
                        output.reverse_depth,
                    )),
                    stencil: stencil_state(state),
                    bias: Default::default(),
                }),
                multisample: output.multisample_state(),
                multiview_mask: None,
                cache: None,
            })
        };
        let format = if output.weighted() {
            None
        } else {
            output.format_override
        };
        let key = (
            mode,
            index,
            output.screen_reflections,
            output.screen_indirect_light,
            output.lightmapped,
            output.weighted(),
            output.model_particles,
            output.fog,
            surface.blend_mode,
            surface.depth_draw,
            surface.cull,
            (
                surface.channel_remapping,
                surface.vertex_colors,
                surface.vertex_masks,
                surface.custom_base_fresnel,
                surface.custom_coat_fresnel,
                surface.anisotropy,
                output.capture_geometry,
                fixed_state(output.pipeline_state),
                output.stenciled,
                output.raster,
                format,
                output.reverse_depth,
            ),
        );
        if let Some(source) = custom
            && let Some(pipeline) =
                self.custom
                    .get(device, (source.0, source.1, wireframe), key, create)
        {
            return pipeline;
        }
        if output.stenciled
            || output.reverse_depth
            || format.is_some()
            || output.pipeline_state != CapturePipelineState::default()
            || output.screen_data()
            || output.capture_geometry.is_some()
            || output.lightmapped
            || output.model_particles
            || output.fog != FogMode::Off
            || surface.blend_mode != MeshBlendMode::SourceOver
            || surface.depth_draw != MeshDepthDraw::OpaqueOnly
            || surface.cull == MeshCullMode::Front
            || surface.channel_remapping
            || !surface.vertex_colors
            || surface.vertex_masks
            || surface.custom_base_fresnel
            || surface.custom_coat_fresnel
            || surface.anisotropy
        {
            self.specialized.entry(key).or_insert_with(|| create(None))
        } else if output.weighted() {
            self.weighted[mode][surface.index() + 8 * deformation as usize]
                .get_or_insert_with(|| create(None))
        } else {
            self.variants[mode][index].get_or_insert_with(|| create(None))
        }
    }

    pub(super) fn material_shader_builds(&self) -> u64 {
        self.custom.builds
    }

    pub(super) fn skybox(
        &mut self,
        device: &wgpu::Device,
        output: ColorOutput,
        ignore_depth: bool,
    ) -> &wgpu::RenderPipeline {
        let stage = if output.screen_indirect_light {
            2
        } else {
            usize::from(output.screen_reflections)
        };
        let ignore_depth = ignore_depth && output.fog != FogMode::Off;
        let index = 2 * (stage + 3 * output.fog as usize)
            + usize::from(output.multisampling)
            + 6 * FogMode::COUNT * usize::from(output.hdr)
            + 12 * FogMode::COUNT * usize::from(ignore_depth)
            + 24 * FogMode::COUNT * usize::from(output.stenciled)
            + 48 * FogMode::COUNT * usize::from(output.reverse_depth);
        let create = || {
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
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("REVERSE_DEVICE_DEPTH", f64::from(output.reverse_depth))],
                        ..Default::default()
                    },
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
                    format: output.depth_format(),
                    depth_write_enabled: Some(false),
                    depth_compare: Some(if output.fog != FogMode::Off && !ignore_depth {
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
        };
        if let Some(format) = output
            .format_override
            .or_else(|| output.reverse_depth.then(|| output.format()))
        {
            self.skybox_formats
                .entry((index, format))
                .or_insert_with(create)
        } else {
            self.skybox[index].get_or_insert_with(create)
        }
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
