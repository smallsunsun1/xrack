use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

use crate::scene3d::{
    COLOR_FORMAT, DEPTH_FORMAT, HDR_FORMAT, INSTANCE_STRIDE,
    color_capture::{CaptureMask, attachment_format},
    material::{
        CLEARCOAT_SETTINGS_BINDING, CLEARCOAT_TEXTURES, FIRST_SHADER_TEXTURE_BINDING,
        MATERIAL_TEXTURES, MaterialProgram, SCALAR_MAP_BINDINGS, SHADER_CAPTURE_BINDING,
        SHADER_SPATIAL_SAMPLER_BINDING, shader_texture_slots, shader_view_dimension,
    },
};
use xrack_runtime::{
    material_shader::{
        CUBEMAP_SAMPLING, EMPTY_AUGMENT_TEXTURES, INTERFACE, MAX_TEXTURES, MaterialHooks,
        VALIDATION_TEXTURES, VERTEX_APPLICATION, capture_augment_parts,
    },
    scene_effect::{
        CAPTURE_DEPTH_READ, CAPTURE_POSITION, CaptureAttachment, CaptureBlend,
        CaptureBlendComponent, CaptureBlendFactor, CaptureBlendOp, CaptureDepthCompare,
        CapturePipelineState, CaptureRenderables, CaptureStencilOp, MATERIAL_DATA_ATTACHMENTS,
        ShaderTextureKinds, write_volume_interface,
    },
    scene3d::{MeshBlendMode, MeshCullMode, MeshDepthDraw, MeshTopology, MeshVertex},
};

use super::deform::{Deformation, DeformationLayouts, SKIN_LAYOUT, shader_source};
use super::transparency::{ACCUMULATION, REVEALAGE};
use super::volumetric_fog::FogMode;
use super::wireframe::{self, RasterMode};
use super::{
    linked_transparency::{self, LinkedSamples},
    screen_lighting::surface_targets,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum VolumeMode {
    Off,
    Volume,
    Dispersion,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum BinormalSource {
    Derived,
    Morph,
    Vertex,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Surface {
    pub wide_points: bool,
    pub wide_lines: bool,
    pub unlit_primitive: bool,
    pub topology: MeshTopology,
    pub blend: bool,
    pub blend_mode: MeshBlendMode,
    pub depth_draw: MeshDepthDraw,
    pub channel_remapping: bool,
    pub separate_surface_maps: bool,
    pub reflection_map: bool,
    pub height_mapped: bool,
    pub tinted_dielectric: bool,
    pub iridescent: bool,
    pub sheen: bool,
    pub vertex_colors: bool,
    pub vertex_masks: bool,
    pub custom_base_fresnel: bool,
    pub custom_coat_fresnel: bool,
    pub transmission: bool,
    pub volume: VolumeMode,
    pub cull: MeshCullMode,
    pub vertex_tangents: bool,
    pub binormals: BinormalSource,
    pub anisotropy: bool,
    pub clearcoat: bool,
}

impl Surface {
    pub(super) fn expanded(self) -> bool {
        self.wide_points || self.wide_lines
    }

    const fn index(self) -> usize {
        4 * self.vertex_tangents as usize
            + 2 * self.blend as usize
            + matches!(self.cull, MeshCullMode::None) as usize
    }
}

pub(super) fn mesh_primitive(
    topology: MeshTopology,
    expanded: bool,
    raster: RasterMode,
    reverse_winding: bool,
    cull: MeshCullMode,
) -> wgpu::PrimitiveState {
    wgpu::PrimitiveState {
        topology: match (topology, expanded) {
            (MeshTopology::Triangles, _) | (_, true) => wgpu::PrimitiveTopology::TriangleList,
            (MeshTopology::Lines, false) => wgpu::PrimitiveTopology::LineList,
            (MeshTopology::Points, false) => wgpu::PrimitiveTopology::PointList,
        },
        front_face: if reverse_winding && !expanded {
            wgpu::FrontFace::Cw
        } else {
            wgpu::FrontFace::Ccw
        },
        cull_mode: if topology == MeshTopology::Triangles {
            culled_face(cull)
        } else {
            None
        },
        polygon_mode: raster.polygon_mode(),
        ..Default::default()
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
    Named(CaptureMask),
    Data(DataTargets),
    Combined(CaptureMask, DataTargets),
}

type DataTargets = [Option<wgpu::TextureFormat>; MATERIAL_DATA_ATTACHMENTS.len()];

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
            Self::Data(data) => attachment
                .material_data_slot()
                .is_some_and(|index| data[index].is_some()),
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

    fn data_formats(self) -> DataTargets {
        match self {
            Self::Data(data) | Self::Combined(_, data) => data,
            _ => [None; MATERIAL_DATA_ATTACHMENTS.len()],
        }
    }

    fn data_targets(self, writes: wgpu::ColorWrites) -> [Option<wgpu::ColorTargetState>; 3] {
        self.data_formats().map(|format| {
            format.map(|format| wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: writes,
            })
        })
    }

    pub(super) fn combined_targets(
        self,
        color: wgpu::ColorTargetState,
        writes: wgpu::ColorWrites,
    ) -> Option<([Option<wgpu::ColorTargetState>; 8], usize)> {
        if !self.combined() {
            return None;
        }
        let [normal, base, emissive, depth] = self.geometry_targets(writes);
        let [data0, data1, data2] = self.data_targets(writes);
        let targets = [
            normal,
            base,
            emissive,
            depth,
            Some(color),
            data0,
            data1,
            data2,
        ];
        let len = targets
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |index| index + 1);
        Some((targets, len))
    }
}

#[derive(Clone, Copy)]
pub(super) struct ColorOutput {
    pub hdr: bool,
    pub format_override: Option<wgpu::TextureFormat>,
    pub encode_srgb: bool,
    pub multisampling: bool,
    pub pass: ColorPass,
    pub renderables: CaptureRenderables,
    pub pipeline_state: CapturePipelineState,
    pub stenciled: bool,
    pub raster: RasterMode,
    pub reverse_winding: bool,
    pub reverse_depth: bool,
    pub lod_fade: bool,
    pub screen_reflections: bool,
    pub screen_indirect_light: bool,
    pub capture_geometry: Option<GeometryOutput>,
    pub named_geometry: bool,
    pub lightmapped: bool,
    pub model_particles: bool,
    pub linked_transparency: bool,
    pub debug_material: bool,
    pub fog: FogMode,
}

impl ColorOutput {
    pub(super) fn writes_material_data(self) -> bool {
        self.capture_geometry
            .is_some_and(|geometry| geometry.data_formats().iter().any(Option::is_some))
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
            Some(GeometryOutput::Data(formats)) => {
                formats.into_iter().flatten().next().unwrap_or(HDR_FORMAT)
            }
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
        attributes: &wgpu::vertex_attr_array![4 => Float32x4, 5 => Float32x4, 6 => Float32x4, 7 => Float32x4, 8 => Uint32x2],
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
    pub capture_augment: Option<Arc<str>>,
    pub capture_augment_slots: usize,
    pub globals: wgpu::BindGroupLayout,
    augment_globals: [Option<wgpu::BindGroupLayout>; MAX_TEXTURES],
    linked_globals: [Option<wgpu::BindGroupLayout>; 2],
    pub materials: [wgpu::BindGroupLayout; 2],
    pub environment: wgpu::BindGroupLayout,
    pub panorama: wgpu::BindGroupLayout,
    pub shader_materials: [[Option<wgpu::BindGroupLayout>; 2]; 2],
    pub deformations: DeformationLayouts,
    layouts: [[wgpu::PipelineLayout; 8]; 2],
    shader: wgpu::ShaderModule,
    wire_shader: Option<wgpu::ShaderModule>,
    line_shaders: HashMap<(Deformation, usize, bool, bool), wgpu::ShaderModule>,
    linked_shaders: [Option<wgpu::ShaderModule>; 4],
    binormal_shaders: [Option<wgpu::ShaderModule>; 6],
    panorama_shaders: [Option<wgpu::ShaderModule>; 12],
    variants: [[Option<wgpu::RenderPipeline>; 64]; 24],
    weighted: [[Option<wgpu::RenderPipeline>; 32]; 24],
    skybox: [Option<wgpu::RenderPipeline>; 48 * FogMode::COUNT],
    skybox_formats: HashMap<(usize, wgpu::TextureFormat, bool), wgpu::RenderPipeline>,
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
        (bool, bool, bool, bool, bool, bool, bool),
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
        (
            bool,
            bool,
            bool,
            bool,
            bool,
            BinormalSource,
            MeshTopology,
            bool,
            bool,
            bool,
        ),
    ),
);

#[derive(Default)]
struct CustomMaterials {
    programs: VecDeque<CustomProgram>,
    builds: u64,
}

struct CustomProgram {
    source: Option<Arc<str>>,
    augment: Option<Arc<str>>,
    slots: (usize, ShaderTextureKinds),
    augment_slots: usize,
    wireframe: bool,
    linked: Option<LinkedSamples>,
    binormals: bool,
    panorama: bool,
    line_mode: Option<Deformation>,
    module: Option<wgpu::ShaderModule>,
    pipelines: HashMap<VariantKey, wgpu::RenderPipeline>,
}

impl CustomMaterials {
    fn get(
        &mut self,
        device: &wgpu::Device,
        (source, augment): (Option<&Arc<str>>, Option<&Arc<str>>),
        (slots, augment_slots): ((usize, ShaderTextureKinds), usize),
        (wireframe, linked, binormals, panorama, line_mode): (
            bool,
            Option<LinkedSamples>,
            bool,
            bool,
            Option<Deformation>,
        ),
        key: VariantKey,
        create: impl Fn(Option<&wgpu::ShaderModule>, bool) -> wgpu::RenderPipeline,
    ) -> Option<&wgpu::RenderPipeline> {
        const RETAINED_PROGRAMS: usize = 16;
        let load = |augment: Option<(&str, &str)>| {
            let built_in;
            let wgsl = if let Some(source) = source {
                source
            } else {
                built_in = MaterialHooks::default().prepare("");
                built_in.as_str()
            };
            let bindings = if augment.is_some() { augment_slots } else { 0 };
            checked_material_gpu(device, || {
                mesh_shader(
                    device,
                    wgsl,
                    slots,
                    (wireframe, linked, binormals, panorama, line_mode),
                    (augment, bindings),
                )
            })
        };
        let index = self.programs.iter().position(|program| {
            program.slots == slots
                && program.augment_slots == augment_slots
                && program.wireframe == wireframe
                && program.linked == linked
                && program.binormals == binormals
                && program.panorama == panorama
                && program.line_mode == line_mode
                && program.source.as_ref() == source
                && program.augment.as_ref() == augment
        });
        if index != Some(0) {
            let program = index
                .and_then(|index| self.programs.remove(index))
                .unwrap_or_else(|| CustomProgram {
                    source: source.cloned(),
                    augment: augment.cloned(),
                    slots,
                    augment_slots,
                    wireframe,
                    linked,
                    binormals,
                    panorama,
                    line_mode,
                    module: load(augment.and_then(|source| capture_augment_parts(source))),
                    pipelines: HashMap::new(),
                });
            self.programs.truncate(RETAINED_PROGRAMS - 1);
            self.programs.push_front(program);
        }
        let program = self.programs.front_mut()?;
        Some(program.pipelines.entry(key).or_insert_with(|| {
            let mut build = |module, augmented| {
                self.builds += 1;
                checked_material_gpu(device, || create(Some(module), augmented))
            };
            program
                .module
                .as_ref()
                .and_then(|module| build(module, augment.is_some()))
                .or_else(|| {
                    // Keep the original vertex and material program if the device rejects the link.
                    if augment.is_some() && source.is_some() {
                        let module = load(None)?;
                        self.builds += 1;
                        checked_material_gpu(device, || create(Some(&module), false))
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| create(None, false))
        }))
    }
}

fn checked_material_gpu<T>(device: &wgpu::Device, build: impl FnOnce() -> T) -> Option<T> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let value = build();
    if let Some(error) = pollster::block_on(scope.pop()) {
        tracing::warn!(%error, "XRack GPU rejected a material program variant");
        None
    } else {
        Some(value)
    }
}

pub(super) const AUGMENT_TEXTURE_BINDING: u32 = 9;
pub(super) const AUGMENT_CAPTURE_BINDING: u32 = AUGMENT_TEXTURE_BINDING + MAX_TEXTURES as u32;
pub(super) const PACKED_SHADOW_BINDING: u32 = AUGMENT_CAPTURE_BINDING + 1;

struct TextureInterface {
    name: &'static str,
    flags: &'static str,
    group: u32,
    textures: u32,
    projections: u32,
}

const MATERIAL_INPUTS: TextureInterface = TextureInterface {
    name: "material",
    flags: "material.shader_texture_flags",
    group: 1,
    textures: FIRST_SHADER_TEXTURE_BINDING,
    projections: SHADER_CAPTURE_BINDING,
};

const AUGMENT_INPUTS: TextureInterface = TextureInterface {
    name: "capture_augment",
    flags: "globals.augment_texture_flags",
    group: 0,
    textures: AUGMENT_TEXTURE_BINDING,
    projections: AUGMENT_CAPTURE_BINDING,
};

fn texture_interface(
    interface: TextureInterface,
    slots: usize,
    positions: bool,
    kinds: ShaderTextureKinds,
) -> String {
    let TextureInterface {
        name,
        flags,
        group,
        textures,
        projections,
    } = interface;
    let bindings: String = (0..slots)
        .map(|slot| {
            let binding = textures + slot as u32;
            let dimension = kinds.wgsl(slot);
            format!(
                "@group({group}) @binding({binding}) var xr_{name}_image_{slot}: texture_{dimension}<f32>;\n"
            )
        })
        .collect();
    let sizes: String = (0..slots)
        .map(|slot| {
            format!("case {slot}u: {{ return textureDimensions(xr_{name}_image_{slot}).xy; }}\n")
        })
        .collect();
    let planar_slots = (0..slots).filter(|slot| kinds.nonplanar() & (1 << slot) == 0);
    let samples: String = planar_slots.clone()
        .map(|slot| format!("case {slot}u: {{ return xr_material_sample(xr_{name}_image_{slot}, uv, flags); }}\n"))
        .collect();
    let samples_lod: String = planar_slots.clone()
        .map(|slot| format!("case {slot}u: {{ return xr_material_sample_lod(xr_{name}_image_{slot}, uv, flags, lod); }}\n"))
        .collect();
    let samples_grad: String = planar_slots.clone()
        .map(|slot| format!("case {slot}u: {{ return xr_material_sample_grad(xr_{name}_image_{slot}, uv, flags, dx, dy); }}\n"))
        .collect();
    let positions = if positions {
        let cases: String = planar_slots.map(|slot| format!(
            "case {slot}u: {{ return xr_material_world_position(xr_{name}_image_{slot}, uv, flags, xr_{name}_inverse_clip[{slot}]); }}\n"
        )).collect();
        format!(
            r#"
@group({group}) @binding({projections}) var<uniform> xr_{name}_inverse_clip: array<mat4x4<f32>, {slots}>;
fn {name}_texture_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition {{
    if !{name}_has_texture(index) || !all(abs(uv) <= vec2(1e20)) {{
        return xr_invalid_capture_position();
    }}
    let flags = {flags}[index];
    switch index {{
        {cases}
        default: {{ return xr_invalid_capture_position(); }}
    }}
}}"#
        )
    } else {
        format!(
            "fn {name}_texture_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition {{ return xr_invalid_capture_position(); }}"
        )
    };
    format!(
        r#"
{bindings}
fn {name}_has_texture(index: u32) -> bool {{
    return index < {slots}u && ({flags}[index] & 1u) != 0u;
}}
fn {name}_texture_size(index: u32) -> vec2<u32> {{
    if !{name}_has_texture(index) {{ return vec2(0u); }}
    switch index {{
        {sizes}
        default: {{ return vec2(0u); }}
    }}
}}
fn {name}_texture(index: u32, uv: vec2<f32>) -> vec4<f32> {{
    if !{name}_has_texture(index) || !all(abs(uv) <= vec2(1e20)) {{
        return vec4(0.0);
    }}
    let flags = {flags}[index];
    switch index {{
        {samples}
        default: {{ return vec4(0.0); }}
    }}
}}
fn {name}_texture_lod(index: u32, uv: vec2<f32>, lod: f32) -> vec4<f32> {{
    if !{name}_has_texture(index) || !all(abs(uv) <= vec2(1e20)) {{
        return vec4(0.0);
    }}
    let flags = {flags}[index];
    switch index {{
        {samples_lod}
        default: {{ return vec4(0.0); }}
    }}
}}
fn {name}_texture_grad(index: u32, uv: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {{
    if !{name}_has_texture(index) || !all(abs(uv) <= vec2(1e20)) {{
        return vec4(0.0);
    }}
    let flags = {flags}[index];
    switch index {{
        {samples_grad}
        default: {{ return vec4(0.0); }}
    }}
}}
{positions}"#
    )
}

fn material_cubemap_interface(slots: usize, kinds: ShaderTextureKinds) -> String {
    let cubemaps = kinds.cubemaps;
    let mut declarations = String::new();
    if kinds.nonplanar() != 0 {
        let sample = if cubemaps != 0 { CUBEMAP_SAMPLING } else { "" };
        declarations = format!(
            "{sample}\n@group(1) @binding({SHADER_SPATIAL_SAMPLER_BINDING}) var xr_material_spatial_sampler: sampler;"
        );
    }
    let cases: String = (0..slots)
        .filter(|slot| cubemaps & (1 << slot) != 0)
        .map(|slot| format!(
            "case {slot}u: {{ return xr_sample_cubemap(xr_material_image_{slot}, xr_material_spatial_sampler, direction, lod); }}\n"
        ))
        .collect();
    format!(
        r#"
{declarations}
fn material_has_cubemap(index: u32) -> bool {{
    return material_has_texture(index) && ({cubemaps}u & (1u << (index & 31u))) != 0u;
}}
fn material_cubemap(index: u32, direction: vec3<f32>) -> vec4<f32> {{
    return material_cubemap_lod(index, direction, 0.0);
}}
fn material_cubemap_lod(index: u32, direction: vec3<f32>, lod: f32) -> vec4<f32> {{
    if !material_has_cubemap(index) {{ return vec4(0.0); }}
    switch index {{
        {cases}
        default: {{ return vec4(0.0); }}
    }}
}}"#
    )
}

fn mesh_shader(
    device: &wgpu::Device,
    custom: &str,
    (slots, kinds): (usize, ShaderTextureKinds),
    (wireframe, linked, binormals, panorama, line_mode): (
        bool,
        Option<LinkedSamples>,
        bool,
        bool,
        Option<Deformation>,
    ),
    (augment, augment_slots): (Option<(&str, &str)>, usize),
) -> wgpu::ShaderModule {
    let standard = concat!(
        include_str!("light_eval.wgsl"),
        "\n",
        include_str!("scene.wgsl")
    );
    let sky_radiance = include_str!("sky_radiance.wgsl");
    let sky_ray = xrack_runtime::scene3d::SKY_RAY_WGSL;
    let scene = if wireframe {
        std::borrow::Cow::Owned(wireframe::mesh(standard))
    } else {
        std::borrow::Cow::Borrowed(standard)
    };
    let scene = if let Some(samples) = linked {
        std::borrow::Cow::Owned(linked_transparency::capture_shader(&scene, samples))
    } else {
        scene
    };
    let scene = if binormals {
        std::borrow::Cow::Owned(
            scene
                .replace(
                    "const EXPLICIT_BINORMALS: bool = false;",
                    "const EXPLICIT_BINORMALS: bool = true;",
                )
                .replace(
                    "// XRack vertex binormal varying",
                    "@location(14) morph_binormal: vec3<f32>,",
                )
                .replace("// XRack vertex binormal rest", "vec3(0.0),")
                .replace(
                    "// XRack write morph binormal",
                    "result.morph_binormal = vec3(
                dot(original.model0.xyz, binormal),
                dot(original.model1.xyz, binormal),
                dot(original.model2.xyz, binormal));",
                )
                .replace(
                    "// XRack read morph binormal\n    return vec3(0.0);",
                    "return vertex.morph_binormal;",
                ),
        )
    } else {
        scene
    };
    let scene = if panorama {
        std::borrow::Cow::Owned(scene
            .replace("var probe: texture_cube<f32>;", "var probe: texture_2d<f32>;")
            .replace("const MATERIAL_REFLECTION_MAP: bool = false;", "const MATERIAL_REFLECTION_MAP: bool = true;")
            .replace("return textureSampleLevel(probe, probe_sampler, lookup, lod).rgb;", r#"
                let sample = textureSampleLevel(probe, probe_sampler, reflection_panorama_uv(lookup), lod);
                if material.reflection_options.x == 0.0 {
                    return sample.rgb;
                }
                let straight = max(vec3(0.0), sample.rgb / max(sample.a, 1e-6));
                if material.reflection_options.x > 1.5 {
                    return srgb_to_linear(min(straight, vec3(1.0)));
                }
                return straight;
            "#))
    } else {
        scene
    };
    let scene = if let Some(mode) = line_mode {
        std::borrow::Cow::Owned(super::primitive_raster::line_shader(&scene, mode))
    } else {
        scene
    };
    let textures = if slots == 0 {
        VALIDATION_TEXTURES.to_owned()
    } else {
        let mut textures = texture_interface(
            MATERIAL_INPUTS,
            slots,
            MaterialHooks::from_source(custom).capture_position,
            kinds,
        ) + &material_cubemap_interface(slots, kinds);
        write_volume_interface(&mut textures, kinds.volumes, true);
        textures
    };
    let augment_textures = if augment_slots == 0 {
        std::borrow::Cow::Borrowed(EMPTY_AUGMENT_TEXTURES)
    } else {
        std::borrow::Cow::Owned(texture_interface(
            AUGMENT_INPUTS,
            augment_slots,
            true,
            ShaderTextureKinds::PLANAR,
        ))
    };
    let texture_sampling = if slots != 0 || augment_slots != 0 {
        concat!(
            "
",
            include_str!("material_sampling.wgsl")
        )
    } else {
        ""
    };
    let (directives, augment) = augment.unwrap_or(("", r#"
        const XRACK_AUGMENT_UV_GRADIENTS: bool = false;
        fn xrack_augment_scene_capture_augment(input: XrMaterialInput, surface: XrMaterialSurface, index: u32) -> vec4<f32> {
            return vec4(0.0);
        }
    "#));
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("XRack 3D mesh shader"),
        source: shader_source(&format!(
            "{directives}\n{custom}\n{augment}\n{INTERFACE}\n{CAPTURE_POSITION}\n{CAPTURE_DEPTH_READ}\n{texture_sampling}\n{textures}\n{augment_textures}\n{VERTEX_APPLICATION}\n{sky_radiance}\n{sky_ray}\n{}\n{}\n{}\n{}\n{}",
            include_str!("screen_packing.wgsl"),
            include_str!("volumetric_fog.wgsl"),
            include_str!("volumetric_fog_profile.wgsl"),
            include_str!("volumetric_fog_noise.wgsl"),
            scene
        )),
    })
}

pub(super) fn material_layout(
    device: &wgpu::Device,
    coated: bool,
    shader_slots: usize,
    positions: bool,
    kinds: ShaderTextureKinds,
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
    bindings.extend(
        SCALAR_MAP_BINDINGS.map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }),
    );
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
        let nonplanar = kinds.nonplanar() & (1 << index) != 0;
        bindings.push(wgpu::BindGroupLayoutEntry {
            binding: FIRST_SHADER_TEXTURE_BINDING + index as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float {
                    filterable: nonplanar,
                },
                view_dimension: shader_view_dimension(kinds, index),
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
    if kinds.nonplanar() != 0 {
        bindings.push(wgpu::BindGroupLayoutEntry {
            binding: SHADER_SPATIAL_SAMPLER_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        });
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

fn global_layout(
    device: &wgpu::Device,
    augment_slots: usize,
    linked: Option<LinkedSamples>,
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
    let mut entries = vec![
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
        wgpu::BindGroupLayoutEntry {
            binding: PACKED_SHADOW_BINDING,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        },
    ];

    if augment_slots != 0 {
        entries.extend((0..augment_slots).map(|slot| wgpu::BindGroupLayoutEntry {
            binding: AUGMENT_TEXTURE_BINDING + slot as u32,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        }));
        entries.push(uniform(
            AUGMENT_CAPTURE_BINDING,
            wgpu::ShaderStages::FRAGMENT,
        ));
    }
    if let Some(samples) = linked {
        entries.extend(linked_transparency::capture_layout_entries(samples));
    }
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("XRack 3D globals layout"),
        entries: &entries,
    })
}

fn globals_layout<'a>(
    base: &'a wgpu::BindGroupLayout,
    augment: &'a [Option<wgpu::BindGroupLayout>],
    linked: Option<&'a wgpu::BindGroupLayout>,
    slots: usize,
    use_linked: bool,
) -> &'a wgpu::BindGroupLayout {
    if use_linked {
        linked.unwrap_or(base)
    } else {
        slots
            .checked_sub(1)
            .and_then(|index| augment.get(index)?.as_ref())
            .unwrap_or(base)
    }
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
        let globals = global_layout(device, 0, None);
        let materials = [false, true]
            .map(|coated| material_layout(device, coated, 0, false, ShaderTextureKinds::PLANAR));
        let shader_materials = [false, true].map(|coated| {
            let slots = shader_texture_slots(device, coated);
            [false, true].map(|positions| {
                (slots > 0).then(|| {
                    material_layout(device, coated, slots, positions, ShaderTextureKinds::PLANAR)
                })
            })
        });
        let environment_layout = |dimension| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("XRack environment layout"),
                entries: &[
                    uniform(0, wgpu::ShaderStages::FRAGMENT),
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: dimension,
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
            })
        };
        let environment = environment_layout(wgpu::TextureViewDimension::Cube);
        let panorama = environment_layout(wgpu::TextureViewDimension::D2);
        let deformations = DeformationLayouts::new(device);
        let layouts = std::array::from_fn(|coat| {
            std::array::from_fn(|index| {
                let (mode, wide) = Deformation::at_pipeline(index);
                let extra = deformations.get_for_line(mode, wide);
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
        let shader = mesh_shader(
            device,
            &MaterialHooks::default().prepare(""),
            (0, ShaderTextureKinds::PLANAR),
            (false, None, false, false, None),
            (None, 0),
        );
        Self {
            globals,
            materials,
            shader_materials,
            environment,
            panorama,
            deformations,
            layouts,
            shader,
            wire_shader: None,
            line_shaders: HashMap::new(),
            linked_shaders: Default::default(),
            binormal_shaders: Default::default(),
            panorama_shaders: Default::default(),
            variants: [const { [const { None }; 64] }; 24],
            weighted: [const { [const { None }; 32] }; 24],
            skybox: [const { None }; 48 * FogMode::COUNT],
            skybox_formats: HashMap::new(),
            specialized: HashMap::new(),
            custom: CustomMaterials::default(),
            capture_augment: None,
            capture_augment_slots: 0,
            augment_globals: Default::default(),
            linked_globals: Default::default(),
        }
    }

    pub(super) fn prepare_augment_layout(&mut self, device: &wgpu::Device, slots: usize) {
        if let Some(index) = slots.checked_sub(1)
            && let Some(layout) = self.augment_globals.get_mut(index)
        {
            layout.get_or_insert_with(|| global_layout(device, slots, None));
        }
    }

    pub(super) fn prepare_linked_layout(&mut self, device: &wgpu::Device, multisampling: bool) {
        let samples = LinkedSamples::from_multisampling(multisampling);
        self.linked_globals[samples.slot()]
            .get_or_insert_with(|| global_layout(device, 0, Some(samples)));
    }

    pub(super) fn globals_layout(
        &self,
        augment_slots: usize,
        linked: bool,
        multisampling: bool,
    ) -> &wgpu::BindGroupLayout {
        globals_layout(
            &self.globals,
            &self.augment_globals,
            self.linked_globals[usize::from(multisampling)].as_ref(),
            augment_slots,
            linked,
        )
    }

    pub(super) fn get(
        &mut self,
        device: &wgpu::Device,
        surface: Surface,
        mut output: ColorOutput,
        deformation: Deformation,
        custom: Option<MaterialProgram<'_>>,
    ) -> &wgpu::RenderPipeline {
        output.raster = output.raster.for_topology(surface.topology);
        let coated = usize::from(surface.clearcoat);
        let mode = usize::from(output.multisampling)
            + 2 * coated
            + 4 * surface.volume as usize
            + 12 * usize::from(output.reverse_winding);
        let index = surface.index() + 8 * usize::from(output.hdr) + 16 * deformation as usize;
        let custom_slots = custom.map_or((0, ShaderTextureKinds::PLANAR), |program| {
            (program.slots, program.kinds)
        });
        let hooks = custom.map_or_else(MaterialHooks::default, |program| {
            MaterialHooks::from_source(program.source)
        });
        let data_formats = output.capture_geometry.map_or(
            [None; MATERIAL_DATA_ATTACHMENTS.len()],
            GeometryOutput::data_formats,
        );
        let mask = |predicate: fn(wgpu::TextureFormat) -> bool| {
            data_formats
                .iter()
                .enumerate()
                .fold(0u8, |mask, (index, format)| {
                    mask | (u8::from(format.is_some_and(predicate)) << index)
                })
        };
        let data_mask = mask(|_| true);
        let full_precision = mask(|format| {
            matches!(
                format,
                wgpu::TextureFormat::R32Float
                    | wgpu::TextureFormat::Rg32Float
                    | wgpu::TextureFormat::Rgba32Float
            )
        });
        let writes_material_data = data_mask != 0;
        let augment = self
            .capture_augment
            .as_ref()
            .filter(|_| writes_material_data);

        let augment_slots = if augment.is_some() {
            self.capture_augment_slots
        } else {
            0
        };
        let linked = output.linked_transparency;
        let globals = globals_layout(
            &self.globals,
            &self.augment_globals,
            self.linked_globals[usize::from(output.multisampling)].as_ref(),
            augment_slots,
            linked,
        );
        let wireframe = output.raster == RasterMode::ShaderLine;
        let linked_samples =
            linked.then(|| LinkedSamples::from_multisampling(output.multisampling));
        let line_mode = surface.wide_lines.then_some(deformation);
        let shader = if let Some(line_mode) = line_mode {
            let binormals = surface.binormals != BinormalSource::Derived;
            let linked_slot = linked_samples.map_or(0, |samples| 1 + samples.slot());
            self.line_shaders
                .entry((line_mode, linked_slot, binormals, surface.reflection_map))
                .or_insert_with(|| {
                    mesh_shader(
                        device,
                        &MaterialHooks::default().prepare(""),
                        (0, ShaderTextureKinds::PLANAR),
                        (
                            false,
                            linked_samples,
                            binormals,
                            surface.reflection_map,
                            Some(line_mode),
                        ),
                        (None, 0),
                    )
                })
        } else if surface.reflection_map {
            let binormals = surface.binormals != BinormalSource::Derived;
            let index = 6 * usize::from(binormals)
                + usize::from(wireframe)
                + linked_samples.map_or(0, |samples| 2 + 2 * samples.slot());
            self.panorama_shaders[index].get_or_insert_with(|| {
                mesh_shader(
                    device,
                    &MaterialHooks::default().prepare(""),
                    (0, ShaderTextureKinds::PLANAR),
                    (wireframe, linked_samples, binormals, true, None),
                    (None, 0),
                )
            })
        } else if surface.binormals != BinormalSource::Derived {
            let mode =
                usize::from(wireframe) + linked_samples.map_or(0, |samples| 2 + 2 * samples.slot());
            self.binormal_shaders[mode].get_or_insert_with(|| {
                mesh_shader(
                    device,
                    &MaterialHooks::default().prepare(""),
                    (0, ShaderTextureKinds::PLANAR),
                    (wireframe, linked_samples, true, false, None),
                    (None, 0),
                )
            })
        } else if let Some(samples) = linked_samples {
            self.linked_shaders[usize::from(wireframe) + 2 * samples.slot()].get_or_insert_with(
                || {
                    mesh_shader(
                        device,
                        &MaterialHooks::default().prepare(""),
                        (0, ShaderTextureKinds::PLANAR),
                        (wireframe, Some(samples), false, false, None),
                        (None, 0),
                    )
                },
            )
        } else if wireframe {
            self.wire_shader.get_or_insert_with(|| {
                mesh_shader(
                    device,
                    &MaterialHooks::default().prepare(""),
                    (0, ShaderTextureKinds::PLANAR),
                    (true, None, false, false, None),
                    (None, 0),
                )
            })
        } else {
            &self.shader
        };
        let material_layout = custom.and_then(|program| program.layout);
        let create = |custom: Option<&wgpu::ShaderModule>, augmented: bool| {
            let custom_fragment =
                custom.filter(|_| augmented || hooks.uses_fragment(writes_material_data));
            let layout = (surface.reflection_map
                || linked
                || material_layout.is_some()
                || augment_slots != 0)
                .then(|| {
                    let extra = self
                        .deformations
                        .get_for_line(deformation, surface.wide_lines);
                    let groups = [
                        Some(globals),
                        Some(material_layout.unwrap_or(&self.materials[coated])),
                        Some(if surface.reflection_map {
                            &self.panorama
                        } else {
                            &self.environment
                        }),
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
                (surface.binormals == BinormalSource::Vertex).then_some(wgpu::VertexBufferLayout {
                    array_stride: 8,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![12 => Snorm16x4],
                }),
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
            let data_targets = output.capture_geometry.and_then(|geometry| {
                matches!(geometry, GeometryOutput::Data(_))
                    .then(|| geometry.data_targets(wgpu::ColorWrites::ALL))
            });
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
            let constants = [
                ("VOLUME_MATERIAL", volume),
                ("DISPERSION_MATERIAL", dispersion),
                ("LOD_FADE", f64::from(output.lod_fade)),
                ("ENCODE_SRGB", f64::from(output.encode_srgb)),
                (
                    "CAPTURE_EFFECT_SURFACE",
                    f64::from(output.capture_geometry.is_some() && !combined),
                ),
                ("CAPTURE_LIT_SURFACE", f64::from(combined)),
                (
                    "CAPTURE_MATERIAL_DATA",
                    f64::from(
                        if (hooks.has_data() || augmented) && custom_fragment.is_some() {
                            data_mask
                        } else {
                            0
                        },
                    ),
                ),
                ("CAPTURE_FULL_PRECISION_DATA", f64::from(full_precision)),
                (
                    "CAPTURE_AUGMENT",
                    f64::from(augmented && custom_fragment.is_some()),
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
                ("UNLIT_PRIMITIVE", f64::from(surface.unlit_primitive)),
                (
                    "NON_TRIANGLE_PRIMITIVE",
                    f64::from(surface.topology != MeshTopology::Triangles),
                ),
                ("HEIGHT_MAPPED", f64::from(surface.height_mapped)),
                ("TINTED_DIELECTRIC", f64::from(surface.tinted_dielectric)),
                ("IRIDESCENT_MATERIAL", f64::from(surface.iridescent)),
                ("SHEEN_MATERIAL", f64::from(surface.sheen)),
                (
                    "SEPARATE_SURFACE_MAPS",
                    f64::from(surface.separate_surface_maps),
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
                        |mode| matches!(mode, CaptureBlend::Multiply | CaptureBlend::Screen),
                    ))),
                ),
                ("BAKED_LIGHTMAP", f64::from(u8::from(output.lightmapped))),
                ("DEBUG_MATERIAL", f64::from(u8::from(output.debug_material))),
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
            ];
            let linked_constants;
            let fragment_constants = if linked {
                linked_constants = constants
                    .into_iter()
                    .chain([
                        ("XR_LINKED_REVERSE_DEPTH", f64::from(output.reverse_depth)),
                        (
                            "XR_LINKED_EQUAL_DEPTH",
                            f64::from(
                                surface.depth_draw == MeshDepthDraw::OpaquePrepass
                                    && material_prepass,
                            ),
                        ),
                    ])
                    .collect::<Vec<_>>();
                linked_constants.as_slice()
            } else {
                &constants
            };
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if surface.blend {
                    "XRack 3D blended meshes"
                } else {
                    "XRack 3D depth-writing meshes"
                }),
                layout: Some(layout.as_ref().unwrap_or(
                    &self.layouts[coated][deformation.pipeline_index(surface.wide_lines)],
                )),
                vertex: wgpu::VertexState {
                    module: custom.filter(|_| hooks.vertex).unwrap_or(shader),
                    entry_point: Some(
                        match (deformation, surface.vertex_tangents, output.lightmapped) {
                            (Deformation::Rigid, true, _)
                                if surface.binormals == BinormalSource::Vertex =>
                            {
                                "vertex_tangent_binormal"
                            }
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
                            ("WIDE_POINTS", f64::from(surface.wide_points)),
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
                        constants: fragment_constants,
                        ..Default::default()
                    },
                    targets: if let Some((targets, len)) = &combined_targets {
                        &targets[..*len]
                    } else if let Some(targets) = &data_targets {
                        let len = targets
                            .iter()
                            .rposition(Option::is_some)
                            .map_or(0, |index| index + 1);
                        &targets[..len]
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
                primitive: mesh_primitive(
                    surface.topology,
                    surface.expanded(),
                    output.raster,
                    output.reverse_winding,
                    state.cull.unwrap_or(surface.cull),
                ),
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
                (
                    surface.channel_remapping,
                    surface.separate_surface_maps,
                    surface.reflection_map,
                    surface.height_mapped,
                    surface.tinted_dielectric,
                    surface.iridescent,
                    surface.sheen,
                ),
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
                (
                    output.reverse_depth,
                    output.lod_fade,
                    output.encode_srgb,
                    output.debug_material,
                    linked,
                    surface.binormals,
                    surface.topology,
                    surface.unlit_primitive,
                    surface.wide_points,
                    surface.wide_lines,
                ),
            ),
        );
        if (custom.is_some() || augment.is_some())
            && let Some(pipeline) = self.custom.get(
                device,
                (custom.map(|program| program.source), augment),
                (custom_slots, augment_slots),
                (
                    wireframe,
                    linked_samples,
                    surface.binormals != BinormalSource::Derived,
                    surface.reflection_map,
                    line_mode,
                ),
                key,
                create,
            )
        {
            return pipeline;
        }
        if linked
            || surface.unlit_primitive
            || surface.topology != MeshTopology::Triangles
            || surface.binormals != BinormalSource::Derived
            || output.stenciled
            || output.reverse_depth
            || output.lod_fade
            || format.is_some()
            || output.pipeline_state != CapturePipelineState::default()
            || output.screen_data()
            || output.capture_geometry.is_some()
            || output.lightmapped
            || output.debug_material
            || output.model_particles
            || output.fog != FogMode::Off
            || surface.blend_mode != MeshBlendMode::SourceOver
            || surface.depth_draw != MeshDepthDraw::OpaqueOnly
            || surface.cull == MeshCullMode::Front
            || surface.channel_remapping
            || surface.separate_surface_maps
            || surface.reflection_map
            || surface.height_mapped
            || surface.tinted_dielectric
            || surface.iridescent
            || surface.sheen
            || !surface.vertex_colors
            || surface.vertex_masks
            || surface.custom_base_fresnel
            || surface.custom_coat_fresnel
            || surface.anisotropy
        {
            self.specialized
                .entry(key)
                .or_insert_with(|| create(None, false))
        } else if output.weighted() {
            self.weighted[mode][surface.index() + 8 * deformation as usize]
                .get_or_insert_with(|| create(None, false))
        } else {
            self.variants[mode][index].get_or_insert_with(|| create(None, false))
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
        screen_sky: bool,
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
                            ("SCREEN_SPACE_SKY", f64::from(screen_sky)),
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
            .or_else(|| (output.reverse_depth || screen_sky).then(|| output.format()))
        {
            self.skybox_formats
                .entry((index, format, screen_sky))
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
