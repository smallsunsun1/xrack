//! WGSL interface shared by asset validation and scene rendering.
use crate::{
    Color, RecordList, Value, VecModel, Vector3D,
    scene_material::{MeshImage, TextureColorSpace, TextureFilter, TextureWrap},
};
use std::{collections::BTreeMap, fmt::Write, sync::Arc};

pub const MAX_SOURCE_BYTES: usize = 64 * 1024;
pub const MAX_BUFFERS: usize = 8;
pub const FIRST_BUFFER_BINDING: u32 = 5;

pub const SURFACE_BINDING: u32 = FIRST_BUFFER_BINDING + 2 * MAX_BUFFERS as u32;
pub const SURFACE_CAMERA_BINDING: u32 = SURFACE_BINDING + 1;
pub const CAPTURE_CAMERA_BINDING: u32 = SURFACE_CAMERA_BINDING + 1;
pub const SURFACE_HEADER: &str = "// XRack effect needs opaque surfaces\n";
pub const CAPTURE_POSITION_HEADER: &str = "// XRack effect needs capture projection\n";

fn without_surface_header(source: &str) -> &str {
    source.strip_prefix(SURFACE_HEADER).unwrap_or(source)
}

fn without_headers(source: &str) -> &str {
    let source = without_surface_header(source);
    source
        .strip_prefix(CAPTURE_POSITION_HEADER)
        .unwrap_or(source)
}

pub fn source_is_valid(source: &str) -> bool {
    let snippet = without_headers(source);
    !snippet.is_empty() && snippet.len() <= MAX_SOURCE_BYTES
}

pub fn needs_surface(source: &str) -> bool {
    source.starts_with(SURFACE_HEADER)
}

pub fn needs_capture_position(source: &str) -> bool {
    without_surface_header(source).starts_with(CAPTURE_POSITION_HEADER)
}

/// Depth is stored in R32Float. Negative samples mark clear pixels and keep
/// the clear value exact, even when it equals a rendered surface's depth.
pub const CAPTURE_DEPTH_READ: &str = r#"
fn xr_capture_depth_value(raw: f32) -> f32 {
    return select(abs(raw), 0.0, raw == -2.0);
}
"#;

pub fn encode_capture_depth_clear(depth: f32) -> f64 {
    match depth {
        0.0 => -2.0,
        _ => -f64::from(depth),
    }
}

/// Returns a decoded position or the same invalid sentinel for both shader APIs.
pub const CAPTURE_POSITION: &str = r#"
struct XrCapturePosition {
    position: vec3<f32>,
    valid: bool,
}

fn xr_invalid_capture_position() -> XrCapturePosition {
    return XrCapturePosition(vec3(0.0), false);
}

fn xr_capture_unproject(pixel: vec2<i32>, size: vec2<u32>, depth: f32,
                        inverse_clip_rows: mat4x4<f32>) -> XrCapturePosition {
    if !(depth >= 0.0 && depth <= 1.0) {
        return xr_invalid_capture_position();
    }
    let center = (vec2<f32>(pixel) + 0.5) / vec2<f32>(size);
    let clip = vec4(center * vec2(2.0, -2.0) + vec2(-1.0, 1.0), depth, 1.0);
    // CPU rows are uploaded as WGSL columns, so use row-vector multiplication.
    let world = clip * inverse_clip_rows;
    if !(abs(world.w) > 1e-12 && all(abs(world) <= vec4(3.402823e38))) {
        return xr_invalid_capture_position();
    }
    let position = world.xyz / world.w;
    if !all(abs(position) <= vec3(3.402823e38)) {
        return xr_invalid_capture_position();
    }
    return XrCapturePosition(position, true);
}
"#;

/// A scene-linear color target rendered from a viewport's scene.
#[derive(Clone, Debug, PartialEq)]
pub struct Capture {
    pub camera: String,
    pub layers: Option<i64>,
    pub scale: f64,
    pub background: Color,
    pub color_format: OutputFormat,
    pub material_data_format: OutputFormat,
    pub combine_attachments: bool,
    pub depth_clear: f32,
    pub stencil_clear: u8,
    pub inputs: CaptureInputs,
    pub renderables: CaptureRenderables,
    pub override_material: Option<Arc<crate::scene3d::CaptureMaterial>>,
    pub pipeline_state: CapturePipelineState,
    pub raster: CaptureRasterSettings,
}

pub struct CaptureDefinition<'a> {
    pub name: &'a str,
    pub camera: &'a str,
    pub layers: Option<i64>,
    pub scale: f64,
    pub background: Color,
    pub color_format: OutputFormat,
    pub material_data_format: OutputFormat,
    pub combine_attachments: bool,
    pub depth_clear: f64,
    pub stencil_clear: i64,
    pub enabled: bool,
    pub color_source: &'a str,
    pub depth_source: &'a str,
    pub stencil_source: &'a str,
    pub renderables: &'a str,
    pub override_material: Option<crate::scene3d::SceneMaterial>,
    pub pipeline_state: CapturePipelineState,
    pub raster: CaptureRasterSettings,
}

impl CaptureDefinition<'_> {
    pub fn prepare(self) -> Option<(String, Capture)> {
        (self.enabled && !self.name.is_empty()).then(|| {
            (
                self.name.to_owned(),
                Capture {
                    camera: self.camera.to_owned(),
                    layers: self.layers,
                    scale: effect_scale(self.scale),
                    background: self.background,
                    color_format: self.color_format,
                    material_data_format: self.material_data_format,
                    combine_attachments: self.combine_attachments,
                    depth_clear: if self.depth_clear.is_finite() {
                        self.depth_clear.clamp(0.0, 1.0) as f32
                    } else {
                        1.0
                    },
                    stencil_clear: self.stencil_clear.clamp(0, 255) as u8,
                    inputs: CaptureInputs::new(
                        self.color_source,
                        self.depth_source,
                        self.stencil_source,
                    ),
                    renderables: CaptureRenderables::parse(self.renderables),
                    override_material: self
                        .override_material
                        .map(crate::scene3d::CaptureMaterial::new),
                    pipeline_state: self.pipeline_state,
                    raster: self.raster,
                },
            )
        })
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub enum CaptureAttachment {
    #[default]
    Color,
    Depth,
    NormalRoughness,
    BaseColorMetalness,
    EmissiveOcclusion,
    Stencil,
    MaterialData,
}

impl CaptureAttachment {
    pub const ALL: [Self; 7] = [
        Self::Color,
        Self::Depth,
        Self::NormalRoughness,
        Self::BaseColorMetalness,
        Self::EmissiveOcclusion,
        Self::Stencil,
        Self::MaterialData,
    ];

    pub fn parse(name: &str) -> Self {
        match name {
            "depth" => Self::Depth,
            "normal-roughness" => Self::NormalRoughness,
            "base-color-metalness" => Self::BaseColorMetalness,
            "emissive-occlusion" => Self::EmissiveOcclusion,
            "material-data" => Self::MaterialData,
            _ => Self::Color,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct CaptureRead {
    pub name: Arc<str>,
    pub attachment: CaptureAttachment,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureInputs {
    pub color: Option<CaptureRead>,
    pub depth: Option<CaptureRead>,
    pub stencil: Option<CaptureRead>,
}

impl CaptureInputs {
    pub fn new(color: &str, depth: &str, stencil: &str) -> Self {
        let read = |name: &str, attachment| {
            (!name.is_empty()).then(|| CaptureRead {
                name: Arc::from(name),
                attachment,
            })
        };
        Self {
            color: read(color, CaptureAttachment::Color),
            depth: read(depth, CaptureAttachment::Depth),
            stencil: read(stencil, CaptureAttachment::Stencil),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &CaptureRead> {
        self.color
            .iter()
            .chain(self.depth.iter())
            .chain(self.stencil.iter())
    }

    pub fn is_empty(&self) -> bool {
        self.color.is_none() && self.depth.is_none() && self.stencil.is_none()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaptureRenderables {
    #[default]
    All,
    Opaque,
    Transparent,
    None,
}

impl CaptureRenderables {
    pub fn parse(value: &str) -> Self {
        match value {
            "opaque" => Self::Opaque,
            "transparent" => Self::Transparent,
            "none" => Self::None,
            _ => Self::All,
        }
    }

    pub fn includes(self, blended: bool) -> bool {
        match self {
            Self::All => true,
            Self::Opaque => !blended,
            Self::Transparent => blended,
            Self::None => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CapturePipelineState {
    pub depth_compare: Option<CaptureDepthCompare>,
    pub depth_write: Option<bool>,
    pub cull: Option<crate::scene3d::MeshCullMode>,
    pub blend: Option<CaptureBlend>,
    pub blend_constant: Option<crate::Color>,
    pub stencil: Option<CaptureStencilState>,
    pub color_write_mask: u8,
    pub wireframe: bool,
}

impl Default for CapturePipelineState {
    fn default() -> Self {
        Self {
            depth_compare: None,
            depth_write: None,
            cull: None,
            blend: None,
            blend_constant: None,
            stencil: None,
            color_write_mask: 15,
            wireframe: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureDepthCompare {
    Never,
    Less,
    LessEqual,
    Equal,
    NotEqual,
    GreaterEqual,
    Greater,
    Always,
}

impl CaptureDepthCompare {
    fn named(value: &str) -> Option<Self> {
        Some(match value {
            "never" => Self::Never,
            "less" => Self::Less,
            "less-equal" => Self::LessEqual,
            "equal" => Self::Equal,
            "not-equal" => Self::NotEqual,
            "greater-equal" => Self::GreaterEqual,
            "greater" => Self::Greater,
            "always" => Self::Always,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureStencilOp {
    Keep,
    Zero,
    Replace,
    Invert,
    IncrementClamp,
    DecrementClamp,
    IncrementWrap,
    DecrementWrap,
}

impl CaptureStencilOp {
    fn named(value: &str) -> Self {
        match value {
            "zero" => Self::Zero,
            "replace" => Self::Replace,
            "invert" => Self::Invert,
            "increment-clamp" => Self::IncrementClamp,
            "decrement-clamp" => Self::DecrementClamp,
            "increment-wrap" => Self::IncrementWrap,
            "decrement-wrap" => Self::DecrementWrap,
            _ => Self::Keep,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CaptureStencilState {
    pub reference: u8,
    pub read_mask: u8,
    pub write_mask: u8,
    pub compare: CaptureDepthCompare,
    pub fail: CaptureStencilOp,
    pub depth_fail: CaptureStencilOp,
    pub pass: CaptureStencilOp,
}

impl CaptureStencilState {
    pub fn new(
        [reference, read_mask, write_mask]: [i64; 3],
        compare: &str,
        [fail, depth_fail, pass]: [&str; 3],
    ) -> Self {
        Self {
            reference: reference.clamp(0, 255) as u8,
            read_mask: read_mask.clamp(0, 255) as u8,
            write_mask: write_mask.clamp(0, 255) as u8,
            compare: CaptureDepthCompare::named(compare).unwrap_or(CaptureDepthCompare::Always),
            fail: CaptureStencilOp::named(fail),
            depth_fail: CaptureStencilOp::named(depth_fail),
            pass: CaptureStencilOp::named(pass),
        }
    }

    fn from_record(record: &crate::RecordValue) -> Option<Self> {
        if record.field("enabled") == Some(&Value::Bool(false)) {
            return None;
        }
        let number = |field, default| match record.field(field) {
            Some(Value::Int(value)) => *value,
            _ => default,
        };
        let text = |field| match record.field(field) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        Some(Self::new(
            [
                number("reference", 0),
                number("read-mask", 255),
                number("write-mask", 255),
            ],
            text("compare"),
            [text("fail"), text("depth-fail"), text("pass")],
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureBlend {
    Replace,
    SourceOver,
    Additive,
    Multiply,
    Screen,
    Custom {
        color: CaptureBlendComponent,
        alpha: CaptureBlendComponent,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureBlendFactor {
    Zero,
    One,
    Src,
    OneMinusSrc,
    SrcAlpha,
    OneMinusSrcAlpha,
    Dst,
    OneMinusDst,
    DstAlpha,
    OneMinusDstAlpha,
    SrcAlphaSaturated,
    Constant,
    OneMinusConstant,
}

impl CaptureBlendFactor {
    fn named(name: &str, default: Self) -> Self {
        match name {
            "zero" => Self::Zero,
            "one" => Self::One,
            "src" => Self::Src,
            "one-minus-src" => Self::OneMinusSrc,
            "src-alpha" => Self::SrcAlpha,
            "one-minus-src-alpha" => Self::OneMinusSrcAlpha,
            "dst" => Self::Dst,
            "one-minus-dst" => Self::OneMinusDst,
            "dst-alpha" => Self::DstAlpha,
            "one-minus-dst-alpha" => Self::OneMinusDstAlpha,
            "src-alpha-saturated" => Self::SrcAlphaSaturated,
            "constant" => Self::Constant,
            "one-minus-constant" => Self::OneMinusConstant,
            _ => default,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaptureBlendOp {
    Add,
    Subtract,
    ReverseSubtract,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CaptureBlendComponent {
    pub source: CaptureBlendFactor,
    pub destination: CaptureBlendFactor,
    pub operation: CaptureBlendOp,
}

impl CaptureBlendComponent {
    fn new([source, destination, operation]: [&str; 3]) -> Self {
        use CaptureBlendFactor::{One, OneMinusSrcAlpha};
        let operation = match operation {
            "subtract" => CaptureBlendOp::Subtract,
            "reverse-subtract" => CaptureBlendOp::ReverseSubtract,
            "min" => CaptureBlendOp::Min,
            "max" => CaptureBlendOp::Max,
            _ => CaptureBlendOp::Add,
        };
        // WebGPU min/max compare the unscaled values and require unit factors.
        let (source, destination) =
            if matches!(operation, CaptureBlendOp::Min | CaptureBlendOp::Max) {
                (One, One)
            } else {
                (
                    CaptureBlendFactor::named(source, One),
                    CaptureBlendFactor::named(destination, OneMinusSrcAlpha),
                )
            };
        Self {
            source,
            destination,
            operation,
        }
    }

    fn uses_constant(self) -> bool {
        [self.source, self.destination].into_iter().any(|factor| {
            matches!(
                factor,
                CaptureBlendFactor::Constant | CaptureBlendFactor::OneMinusConstant
            )
        })
    }
}

#[derive(Clone, Copy)]
pub struct CaptureTargetBlend {
    blend: CaptureBlend,
    constant: Option<crate::Color>,
    write_mask: u8,
}

impl CaptureTargetBlend {
    pub fn new(
        blend_enabled: bool,
        write_mask: i64,
        color: [&str; 3],
        alpha: [&str; 3],
        constant: crate::Color,
    ) -> Self {
        let color = CaptureBlendComponent::new(color);
        let alpha = CaptureBlendComponent::new(alpha);
        Self {
            blend: if blend_enabled {
                CaptureBlend::Custom { color, alpha }
            } else {
                CaptureBlend::Replace
            },
            constant: (blend_enabled && (color.uses_constant() || alpha.uses_constant()))
                .then_some(constant),
            write_mask: write_mask.clamp(0, 15) as u8,
        }
    }

    fn from_record(record: &crate::RecordValue) -> Option<Self> {
        if record.field("enabled") == Some(&Value::Bool(false)) {
            return None;
        }
        let text = |field| match record.field(field) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        Some(Self::new(
            record.field("blend-enabled") != Some(&Value::Bool(false)),
            match record.field("write-mask") {
                Some(Value::Int(mask)) => *mask,
                _ => 15,
            },
            ["source-color", "destination-color", "color-operation"].map(text),
            ["source-alpha", "destination-alpha", "alpha-operation"].map(text),
            match record.field("constant") {
                Some(Value::Color(color)) => *color,
                _ => crate::Color(0),
            },
        ))
    }
}

impl CapturePipelineState {
    pub fn new(
        depth_test: Option<bool>,
        depth_write: Option<bool>,
        depth_compare: &str,
        cull: &str,
        blend: &str,
    ) -> Self {
        let depth_compare = if depth_test == Some(false) {
            Some(CaptureDepthCompare::Always)
        } else {
            CaptureDepthCompare::named(depth_compare)
        };
        Self {
            depth_compare,
            depth_write,
            cull: crate::scene3d::MeshCullMode::named(cull),
            blend: match blend {
                "replace" => Some(CaptureBlend::Replace),
                "source-over" => Some(CaptureBlend::SourceOver),
                "additive" => Some(CaptureBlend::Additive),
                "multiply" => Some(CaptureBlend::Multiply),
                "screen" => Some(CaptureBlend::Screen),
                _ => None,
            },
            ..Self::default()
        }
    }

    pub fn with_stencil(mut self, stencil: Option<CaptureStencilState>, color_write: bool) -> Self {
        self.stencil = stencil;
        if !color_write {
            self.color_write_mask = 0;
        }
        self
    }

    pub fn with_target_blend(mut self, target: Option<CaptureTargetBlend>) -> Self {
        if let Some(target) = target {
            self.blend = Some(target.blend);
            self.blend_constant = target.constant;
            self.color_write_mask &= target.write_mask;
        }
        self
    }

    pub fn with_polygon_mode(mut self, mode: &str) -> Self {
        self.wireframe = mode == "line";
        self
    }

    fn from_record(record: &crate::RecordValue) -> (Self, CaptureRasterSettings) {
        let boolean = |field| match record.field(field) {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        };
        let text = |field| match record.field(field) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        if boolean("enabled") == Some(false) {
            return Default::default();
        }
        let rect = |name| match record.field(name) {
            Some(Value::Record(rect)) => CaptureRect::from_record(rect),
            _ => None,
        };
        (
            Self::new(
                boolean("depth-test"),
                boolean("depth-write"),
                text("depth-compare"),
                text("cull-mode"),
                text("blend-mode"),
            )
            .with_stencil(
                match record.field("stencil") {
                    Some(Value::Record(stencil)) => CaptureStencilState::from_record(stencil),
                    _ => None,
                },
                boolean("color-write") != Some(false),
            )
            .with_target_blend(match record.field("target-blend") {
                Some(Value::Record(target)) => CaptureTargetBlend::from_record(target),
                _ => None,
            })
            .with_polygon_mode(text("polygon-mode")),
            CaptureRasterSettings {
                viewport: rect("viewport"),
                scissor: rect("scissor"),
            },
        )
    }
}

/// Rectangles in normalized capture coordinates, with a top-left origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CaptureRasterSettings {
    pub viewport: Option<CaptureRect>,
    pub scissor: Option<CaptureRect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaptureRect([f32; 4]);

impl CaptureRect {
    pub const FULL: Self = Self([0.0, 0.0, 1.0, 1.0]);

    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        if ![x, y, width, height].into_iter().all(f64::is_finite) || width <= 0.0 || height <= 0.0 {
            return Self([0.0; 4]);
        }
        Self([x, y, x + width, y + height].map(|edge| edge.clamp(0.0, 1.0) as f32))
    }

    pub const fn bounds(self) -> [f32; 4] {
        self.0
    }

    pub fn intersect(self, other: Self) -> Self {
        let [left, top, right, bottom] = self.0;
        let [x0, y0, x1, y1] = other.0;
        Self([left.max(x0), top.max(y0), right.min(x1), bottom.min(y1)])
    }

    fn from_record(record: &crate::RecordValue) -> Option<Self> {
        if record.field("enabled") == Some(&Value::Bool(false)) {
            return None;
        }
        let number = |field| match record.field(field) {
            Some(Value::Float(value)) => *value,
            _ => 0.0,
        };
        Some(Self::new(
            number("x"),
            number("y"),
            number("width"),
            number("height"),
        ))
    }
}

pub type Captures = BTreeMap<String, Capture>;

pub fn model_captures<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<(String, Capture)>,
) -> Arc<Captures> {
    Arc::new(
        (0..model.len())
            .filter_map(|index| project(model.row(index)?))
            .collect(),
    )
}

pub fn record_captures(
    model: &RecordList,
    textures: impl Fn(&str) -> Option<Arc<MeshImage>>,
    shaders: impl Fn(&str) -> Option<Arc<str>>,
) -> Arc<Captures> {
    model_captures(model.model(), |row| {
        let (pipeline_state, raster) = match row.field("pipeline-state") {
            Some(Value::Record(state)) => CapturePipelineState::from_record(state),
            _ => Default::default(),
        };
        let text = |field| match row.field(field) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        CaptureDefinition {
            name: text("name"),
            camera: text("camera"),
            layers: match row.field("layers") {
                Some(Value::Int(value)) => Some(*value),
                _ => None,
            },
            scale: match row.field("scale") {
                Some(Value::Float(value)) => *value,
                _ => 1.0,
            },
            background: match row.field("background") {
                Some(Value::Color(value)) => *value,
                _ => Color(0),
            },
            color_format: OutputFormat::parse(text("color-format")),
            material_data_format: OutputFormat::parse(text("material-data-format")),
            combine_attachments: row.field("combine-attachments") == Some(&Value::Bool(true)),
            depth_clear: match row.field("depth-clear") {
                Some(Value::Float(value)) => *value,
                _ => 1.0,
            },
            stencil_clear: match row.field("stencil-clear") {
                Some(Value::Int(value)) => *value,
                _ => 0,
            },
            enabled: row.field("enabled") != Some(&Value::Bool(false)),
            color_source: text("color-source"),
            depth_source: text("depth-source"),
            stencil_source: text("stencil-source"),
            renderables: text("renderables"),
            override_material: match row.field("override-material") {
                Some(Value::Record(material)) => {
                    crate::scene3d::SceneMaterial::from_record(material, &textures, &shaders)
                }
                _ => None,
            },
            pipeline_state,
            raster,
        }
        .prepare()
    })
}

fn effect_scale(scale: f64) -> f64 {
    if scale.is_finite() {
        scale.clamp(0.25, 1.0)
    } else {
        1.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputFormat {
    R8,
    R16Float,
    Rg8,
    Rg16Float,
    Rgba8,
    #[default]
    Rgba16Float,
}

impl OutputFormat {
    pub fn parse(name: &str) -> Self {
        match name {
            "r8" => Self::R8,
            "r16f" => Self::R16Float,
            "rg8" => Self::Rg8,
            "rg16f" => Self::Rg16Float,
            "rgba8" => Self::Rgba8,
            _ => Self::Rgba16Float,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BufferSampling {
    pub filter: TextureFilter,
    pub wrap: [TextureWrap; 2],
}

impl BufferSampling {
    pub const DEFAULT: Self = Self {
        filter: TextureFilter::Linear,
        wrap: [TextureWrap::Clamp; 2],
    };

    pub fn new(filter: &str, wrap_u: &str, wrap_v: &str) -> Self {
        Self {
            filter: TextureFilter::parse(filter),
            wrap: [wrap_u, wrap_v]
                .map(|mode| TextureWrap::parse(mode).unwrap_or(TextureWrap::Clamp)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pass {
    pub source: Arc<str>,
    pub strength: f32,
    pub depth: bool,
    pub scale: f64,
    pub parameters: [f32; 3],
    pub format: OutputFormat,
    output: String,
    input: String,
    buffers: Vec<BufferInput>,
    history: bool,
}

impl Pass {
    pub fn new(
        source: String,
        strength: f64,
        depth: bool,
        scale: f64,
        parameters: Vector3D,
    ) -> Option<Self> {
        let strength = strength.clamp(0.0, 1.0) as f32;
        if !source_is_valid(&source) || strength <= 0.0 || strength.is_nan() {
            return None;
        }
        Some(Self {
            source: source.into(),
            strength,
            depth,
            scale: effect_scale(scale),
            parameters: [parameters.x, parameters.y, parameters.z].map(|value| {
                let value = value as f32;
                if value.is_finite() { value } else { 0.0 }
            }),
            format: OutputFormat::default(),
            output: String::new(),
            input: String::new(),
            buffers: Vec::new(),
            history: false,
        })
    }

    pub fn with_buffers(
        mut self,
        output: String,
        input: String,
        mut buffers: Vec<BufferInput>,
    ) -> Self {
        buffers.truncate(MAX_BUFFERS);
        self.output = output;
        self.input = input;
        self.buffers = buffers;
        self
    }

    pub fn with_history(mut self, history: bool) -> Self {
        self.history = history;
        self
    }

    pub fn with_output_format(mut self, name: &str) -> Self {
        self.format = OutputFormat::parse(name);
        self
    }

    pub fn output(&self) -> &str {
        &self.output
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BufferInput {
    source: BufferInputSource,
    pub sampling: BufferSampling,
}

pub enum ExternalTexture<'a> {
    Image(&'a EffectImage),
    Item(&'a str),
    Capture(&'a CaptureRead),
}

impl BufferInput {
    pub fn external_texture(&self) -> Option<ExternalTexture<'_>> {
        match &self.source {
            BufferInputSource::Image(image) => Some(ExternalTexture::Image(image)),
            BufferInputSource::Item(item) => Some(ExternalTexture::Item(item)),
            BufferInputSource::Capture(name) => Some(ExternalTexture::Capture(name)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BufferInputSource {
    Named { name: String, previous_frame: bool },
    Image(EffectImage),
    Item(Arc<str>),
    Capture(CaptureRead),
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct EffectImage {
    pub image: Arc<MeshImage>,
    pub color: TextureColorSpace,
}

impl PartialEq for EffectImage {
    fn eq(&self, other: &Self) -> bool {
        self.color == other.color && Arc::ptr_eq(&self.image, &other.image)
    }
}
impl Eq for EffectImage {}

pub struct BufferInputDefinition<'a> {
    pub name: String,
    pub previous_frame: bool,
    pub texture: &'a str,
    pub item: &'a str,
    pub capture: &'a str,
    pub attachment: CaptureAttachment,
    pub color_space: &'a str,
    pub enabled: bool,
    pub sampling: BufferSampling,
}

impl BufferInputDefinition<'_> {
    pub fn prepare(self, resolve: impl FnOnce(&str) -> Option<Arc<MeshImage>>) -> BufferInput {
        let source = if !self.enabled {
            BufferInputSource::Unavailable
        } else if !self.capture.is_empty() {
            BufferInputSource::Capture(CaptureRead {
                name: self.capture.into(),
                attachment: self.attachment,
            })
        } else if !self.texture.is_empty() {
            resolve(self.texture).map_or(BufferInputSource::Unavailable, |image| {
                let color = if self.color_space == "linear" {
                    TextureColorSpace::Linear
                } else {
                    TextureColorSpace::Srgb
                };
                BufferInputSource::Image(EffectImage { image, color })
            })
        } else if !self.item.is_empty() {
            BufferInputSource::Item(self.item.into())
        } else {
            BufferInputSource::Named {
                name: self.name,
                previous_frame: self.previous_frame,
            }
        };
        BufferInput {
            source,
            sampling: self.sampling,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    passes: Box<[Pass]>,
    routes: Box<[Route]>,
    history_writers: Box<[usize]>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Previous,
    Original,
    Named(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferSource {
    Current(usize),
    History(usize),
    Image(EffectImage),
    Item(Arc<str>),
    Capture(CaptureRead),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferBinding {
    pub source: Option<BufferSource>,
    pub sampling: BufferSampling,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    pub input: Input,
    pub buffers: Box<[BufferBinding]>,
    pub last_read: usize,
}

impl Chain {
    fn new(passes: Box<[Pass]>) -> Self {
        let history: BTreeMap<_, _> = passes
            .iter()
            .enumerate()
            .filter_map(|(index, pass)| {
                (pass.history && !pass.output.is_empty()).then_some((pass.output.as_str(), index))
            })
            .collect();
        let mut writers: Vec<_> = history.values().copied().collect();
        writers.sort_unstable();
        if passes
            .iter()
            .all(|pass| pass.input.is_empty() && pass.buffers.is_empty())
        {
            return Self {
                passes,
                routes: Box::default(),
                history_writers: writers.into_boxed_slice(),
            };
        }
        let mut producers = BTreeMap::<&str, usize>::new();
        let mut routes = Vec::<Route>::with_capacity(passes.len());
        for (index, pass) in passes.iter().enumerate() {
            let mut resolve = |name: &str| {
                let producer = *producers.get(name)?;
                routes[producer].last_read = index;
                Some(producer)
            };
            let input = if pass.input.is_empty() {
                Input::Previous
            } else {
                resolve(&pass.input).map_or(Input::Original, Input::Named)
            };
            let buffers = pass
                .buffers
                .iter()
                .map(|buffer| BufferBinding {
                    source: match &buffer.source {
                        BufferInputSource::Named {
                            name,
                            previous_frame: true,
                        } => history
                            .get(name.as_str())
                            .copied()
                            .map(BufferSource::History),
                        BufferInputSource::Named {
                            name,
                            previous_frame: false,
                        } => resolve(name).map(BufferSource::Current),
                        BufferInputSource::Image(image) => Some(BufferSource::Image(image.clone())),
                        BufferInputSource::Item(item) => Some(BufferSource::Item(item.clone())),
                        BufferInputSource::Capture(name) => {
                            Some(BufferSource::Capture(name.clone()))
                        }
                        BufferInputSource::Unavailable => None,
                    },
                    sampling: buffer.sampling,
                })
                .collect();
            routes.push(Route {
                input,
                buffers,
                last_read: index,
            });
            if !pass.output.is_empty() {
                producers.insert(&pass.output, index);
            }
        }
        Self {
            passes,
            routes: routes.into_boxed_slice(),
            history_writers: writers.into_boxed_slice(),
        }
    }

    pub fn passes(&self) -> &[Pass] {
        &self.passes
    }

    pub fn route(&self, index: usize) -> Option<&Route> {
        self.routes.get(index)
    }

    pub fn history_writers(&self) -> &[usize] {
        &self.history_writers
    }

    pub fn capture_sources(&self) -> impl Iterator<Item = &CaptureRead> {
        self.routes
            .iter()
            .flat_map(|route| route.buffers.iter())
            .filter_map(|buffer| match buffer.source.as_ref() {
                Some(BufferSource::Capture(read)) => Some(read),
                _ => None,
            })
    }

    pub fn item_sources(&self) -> impl Iterator<Item = &str> {
        self.routes
            .iter()
            .flat_map(|route| route.buffers.iter())
            .filter_map(|buffer| match buffer.source.as_ref() {
                Some(BufferSource::Item(name)) => Some(name.as_ref()),
                _ => None,
            })
    }
}

pub fn model_passes<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<Pass>,
) -> Arc<Chain> {
    Arc::new(Chain::new(
        (0..model.len())
            .filter_map(|index| project(model.row(index)?))
            .collect(),
    ))
}

pub fn model_buffer_inputs<T: Clone>(
    model: &VecModel<T>,
    limit: usize,
    input: impl Fn(T) -> BufferInput,
) -> Vec<BufferInput> {
    (0..model.len().min(limit))
        .map(|index| {
            model.row(index).map_or_else(
                || BufferInput {
                    source: BufferInputSource::Unavailable,
                    sampling: BufferSampling::DEFAULT,
                },
                &input,
            )
        })
        .collect()
}

pub fn record_buffer_inputs(
    buffers: &RecordList,
    limit: usize,
    resolve: &impl Fn(&str) -> Option<Arc<MeshImage>>,
) -> Vec<BufferInput> {
    model_buffer_inputs(buffers.model(), limit, |item| {
        let text = |field| match item.field(field) {
            Some(Value::String(value)) => value.as_str(),
            _ => "",
        };
        BufferInputDefinition {
            name: text("name").to_owned(),
            previous_frame: item.field("previous-frame") == Some(&Value::Bool(true)),
            texture: text("texture"),
            item: text("item"),
            capture: text("capture"),
            attachment: CaptureAttachment::parse(text("attachment")),
            color_space: text("color-space"),
            enabled: item.field("enabled") != Some(&Value::Bool(false)),
            sampling: BufferSampling::new(text("filter"), text("wrap-u"), text("wrap-v")),
        }
        .prepare(resolve)
    })
}

pub fn record_passes(
    model: &RecordList,
    resolve: impl Fn(&str) -> Option<Arc<MeshImage>>,
) -> Arc<Chain> {
    model_passes(model.model(), |row| {
        let Value::String(source) = row.field("shader")? else {
            return None;
        };
        let number = |name| match row.field(name) {
            Some(Value::Float(value)) => *value,
            _ => 1.0,
        };
        let parameters = match row.field("parameters") {
            Some(Value::Vector3D(parameters)) => *parameters,
            _ => Vector3D::default(),
        };
        let pass = Pass::new(
            source.clone(),
            number("strength"),
            row.field("depth") == Some(&Value::Bool(true)),
            number("scale"),
            parameters,
        )?;
        let text = |field| match row.field(field) {
            Some(Value::String(value)) => value.clone(),
            _ => String::new(),
        };
        let buffers = match row.field("buffers") {
            Some(Value::RecordList(buffers)) => {
                record_buffer_inputs(buffers, MAX_BUFFERS, &resolve)
            }
            _ => Vec::new(),
        };
        Some(
            pass.with_buffers(text("output"), text("input"), buffers)
                .with_history(row.field("history") == Some(&Value::Bool(true)))
                .with_output_format(&text("output-format")),
        )
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DepthInput {
    None,
    Single,
    Multisampled,
    Surface,
}

pub fn shader(source: &str, depth: DepthInput, buffers: usize, nearest_buffers: u8) -> String {
    // Keep the application's snippet first so WGSL errors retain file line numbers.
    let surface = depth == DepthInput::Surface;
    let depth = match depth {
        DepthInput::None => "fn scene_depth(uv: vec2<f32>) -> f32 { return 1.0; }",
        DepthInput::Single | DepthInput::Surface => {
            r#"
@group(0) @binding(3) var xr_effect_depth: texture_depth_2d;
fn scene_depth(uv: vec2<f32>) -> f32 {
    return textureLoad(xr_effect_depth, xr_effect_pixel(uv, textureDimensions(xr_effect_depth)), 0);
}
"#
        }
        DepthInput::Multisampled => {
            r#"
@group(0) @binding(3) var xr_effect_depth: texture_depth_multisampled_2d;
fn scene_depth(uv: vec2<f32>) -> f32 {
    let pixel = xr_effect_pixel(uv, textureDimensions(xr_effect_depth));
    var nearest = select(1.0, 0.0, XR_EFFECT_REVERSE_DEPTH);
    for (var i = 0u; i < textureNumSamples(xr_effect_depth); i++) {
        let value = textureLoad(xr_effect_depth, pixel, i32(i));
        nearest = select(min(nearest, value), max(nearest, value), XR_EFFECT_REVERSE_DEPTH);
    }
    return nearest;
}
"#
        }
    };
    let positions = needs_capture_position(source) && nearest_buffers != 0;
    let source = without_headers(source);
    let mut shader = format!("{source}\n{INTERFACE}\n{depth}\n{CAPTURE_POSITION}\n");
    let buffers = buffers.min(MAX_BUFFERS);
    write_buffer_interface(&mut shader, buffers, nearest_buffers);
    write_capture_positions(
        &mut shader,
        buffers,
        if positions { nearest_buffers } else { 0 },
    );
    shader.push_str(SURFACE_TYPE);
    if surface {
        let _ = writeln!(
            shader,
            "struct XrEffectProjection {{ inverse_clip: mat4x4<f32>, depth_clear: vec4<f32>, }}\n@group(0) @binding({SURFACE_BINDING}) var xr_effect_surface: texture_2d<f32>;\n@group(0) @binding({SURFACE_CAMERA_BINDING}) var<uniform> xr_effect_projection: XrEffectProjection;"
        );
        shader.push_str(SURFACE_INTERFACE);
    } else {
        shader.push_str(SURFACE_FALLBACK);
    }
    shader
}

fn write_buffer_interface(shader: &mut String, count: usize, nearest_buffers: u8) {
    if nearest_buffers != 0 {
        shader.push_str(CAPTURE_DEPTH_READ);
    }
    for index in 0..count {
        let (texture, sampler) = buffer_bindings(index);
        let _ = writeln!(
            shader,
            "@group(0) @binding({texture}) var xr_effect_buffer_{index}: texture_2d<f32>;\n@group(0) @binding({sampler}) var xr_effect_buffer_sampler_{index}: sampler;"
        );
    }
    let _ = writeln!(
        shader,
        "fn scene_has_buffer(index: u32) -> bool {{ return index < {count}u && (xr_effect_uniform.buffers & (1u << (index & 31u))) != 0u; }}"
    );
    shader.push_str("fn scene_buffer_color(index: u32, uv: vec2<f32>) -> vec4<f32> {\n");
    shader.push_str("  if !scene_has_buffer(index) { return vec4(0.0); }\n  switch index {\n");
    for index in 0..count {
        let texture = format!("xr_effect_buffer_{index}");
        if nearest_buffers & (1 << index) != 0 {
            let _ = writeln!(shader, "    case {index}u: {{
                let sample = textureLoad({texture}, xr_effect_pixel(uv, textureDimensions({texture})), 0);
                return vec4(xr_capture_depth_value(sample.x), sample.yzw);
            }}");
        } else {
            let _ = writeln!(shader, "    case {index}u: {{
                return xr_effect_decode_buffer(index, textureSampleLevel({texture}, xr_effect_buffer_sampler_{index}, uv, 0.0));
            }}");
        }
    }
    shader.push_str("    default: { return vec4(0.0); }\n  }\n}\n");
    shader.push_str("fn scene_buffer_size(index: u32) -> vec2<f32> {\n");
    shader.push_str("  if !scene_has_buffer(index) { return vec2(0.0); }\n  switch index {\n");
    for index in 0..count {
        let _ = writeln!(
            shader,
            "    case {index}u: {{ return vec2<f32>(textureDimensions(xr_effect_buffer_{index})); }}"
        );
    }
    shader.push_str("    default: { return vec2(0.0); }\n  }\n}\n");
}

fn write_capture_positions(shader: &mut String, count: usize, mask: u8) {
    if mask == 0 {
        shader.push_str(
            r#"
fn scene_buffer_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition {
    return xr_invalid_capture_position();
}
"#,
        );
        return;
    }
    let _ = writeln!(
        shader,
        "@group(0) @binding({CAPTURE_CAMERA_BINDING}) var<uniform> xr_buffer_inverse_clip: array<mat4x4<f32>, {count}>;"
    );
    shader.push_str(
        r#"
fn scene_buffer_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition {
    if !scene_has_buffer(index) || !all(abs(uv) <= vec2(1e20)) {
        return xr_invalid_capture_position();
    }
    switch index {
"#,
    );
    for index in (0..count).filter(|&i| mask & (1 << i) != 0) {
        let texture = format!("xr_effect_buffer_{index}");
        let _ = writeln!(shader, "case {index}u: {{
            let size = textureDimensions({texture});
            let pixel = xr_effect_pixel(uv, size);
            return xr_capture_unproject(pixel, size, textureLoad({texture}, pixel, 0).x, xr_buffer_inverse_clip[{index}]);
        }}");
    }
    shader.push_str("    default: { return xr_invalid_capture_position(); }\n    }\n}\n");
}

pub const fn buffer_bindings(index: usize) -> (u32, u32) {
    let texture = FIRST_BUFFER_BINDING + 2 * index as u32;
    (texture, texture + 1)
}

const SURFACE_TYPE: &str = r#"
struct XrEffectSurface {
    position: vec3<f32>,
    normal: vec3<f32>,
    roughness: f32,
    depth: f32,
    valid: bool,
}
"#;

const SURFACE_FALLBACK: &str = r#"
fn scene_surface(uv: vec2<f32>) -> XrEffectSurface {
    return XrEffectSurface(vec3(0.0), vec3(0.0), 1.0, 1.0, false);
}
"#;

const SURFACE_INTERFACE: &str = r#"
fn scene_surface(uv: vec2<f32>) -> XrEffectSurface {
    let dimensions = textureDimensions(xr_effect_surface);
    let pixel = xr_effect_pixel(uv, dimensions);
    let depth = textureLoad(xr_effect_depth, pixel, 0);
    if depth == xr_effect_projection.depth_clear.x {
        return XrEffectSurface(vec3(0.0), vec3(0.0), 1.0, depth, false);
    }
    let point = xr_capture_unproject(pixel, dimensions, depth, xr_effect_projection.inverse_clip);
    if !point.valid {
        return XrEffectSurface(vec3(0.0), vec3(0.0), 1.0, depth, false);
    }
    let encoded = textureLoad(xr_effect_surface, pixel, 0);
    return XrEffectSurface(point.position, normalize(encoded.xyz), encoded.w, depth, true);
}
"#;

const INTERFACE: &str = r#"
override XR_EFFECT_DATA: bool = false;
override XR_EFFECT_REVERSE_DEPTH: bool = false;

struct XrEffectUniform {
    size: vec2<f32>,
    strength: f32,
    buffers: u32,
    parameters: vec4<f32>,
    encoding: u32,
}

@group(0) @binding(0) var xr_effect_color: texture_2d<f32>;
@group(0) @binding(1) var xr_effect_sampler: sampler;
@group(0) @binding(2) var<uniform> xr_effect_uniform: XrEffectUniform;
@group(0) @binding(4) var xr_effect_original: texture_2d<f32>;

fn scene_size() -> vec2<f32> {
    return xr_effect_uniform.size;
}

fn scene_input_size() -> vec2<f32> {
    return vec2<f32>(textureDimensions(xr_effect_color));
}

fn scene_effect_parameters() -> vec3<f32> {
    return xr_effect_uniform.parameters.xyz;
}

fn xr_effect_straight_color(rgba: vec4<f32>) -> vec4<f32> {
    return vec4(rgba.rgb / max(rgba.a, 0.000001), rgba.a);
}

fn xr_effect_decode_buffer(index: u32, rgba: vec4<f32>) -> vec4<f32> {
    let encoding = (xr_effect_uniform.encoding >> (2u * index)) & 3u;
    if encoding == 1u { return rgba; }
    let straight = xr_effect_straight_color(rgba);
    if encoding != 2u { return straight; }
    let encoded = clamp(straight.rgb, vec3(0.0), vec3(1.0));
    let linear = select(encoded / 12.92, pow((encoded + 0.055) / 1.055, vec3(2.4)),
                        encoded > vec3(0.04045));
    return vec4(linear, straight.a);
}

fn scene_color(uv: vec2<f32>) -> vec4<f32> {
    return xr_effect_straight_color(textureSampleLevel(xr_effect_color, xr_effect_sampler, uv, 0.0));
}

fn scene_original_color(uv: vec2<f32>) -> vec4<f32> {
    return xr_effect_straight_color(textureSampleLevel(xr_effect_original, xr_effect_sampler, uv, 0.0));
}

fn xr_effect_pixel(uv: vec2<f32>, dimensions: vec2<u32>) -> vec2<i32> {
    let last = vec2<i32>(dimensions) - vec2(1);
    return clamp(vec2<i32>(uv * vec2<f32>(dimensions)), vec2(0), last);
}

@vertex fn xr_effect_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let vertex = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0))[index];
    return vec4(vertex, 0.0, 1.0);
}

@fragment fn xr_effect_fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = position.xy / scene_size();
    let filtered = scene_effect(uv);
    let alpha = clamp(filtered.a, 0.0, 1.0);
    let output = select(
        vec4(max(filtered.rgb, vec3(0.0)) * alpha, alpha), filtered, XR_EFFECT_DATA
    );
    if xr_effect_uniform.strength >= 1.0 {
        return output;
    }
    var input: vec4<f32>;
    if all(scene_input_size() == scene_size()) {
        input = textureLoad(xr_effect_color, vec2<i32>(position.xy), 0);
    } else {
        input = textureSampleLevel(xr_effect_color, xr_effect_sampler, uv, 0.0);
    }
    return mix(input, output, xr_effect_uniform.strength);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_effect_routes_track_the_previous_version_and_last_read() {
        let pass = |output: &str, input: &str, buffers: &[&str]| {
            Pass::new("effect".into(), 1.0, false, 1.0, Vector3D::default())
                .expect("enabled pass")
                .with_buffers(
                    output.into(),
                    input.into(),
                    buffers
                        .iter()
                        .map(|name| BufferInput {
                            source: BufferInputSource::Named {
                                name: (*name).into(),
                                previous_frame: false,
                            },
                            sampling: BufferSampling::DEFAULT,
                        })
                        .collect(),
                )
        };
        let chain = Chain::new(Box::new([
            pass("soft", "", &[]),
            pass("soft", "soft", &[]),
            pass("unused", "later", &["soft", "missing", "soft"]),
            pass("later", "", &[]),
        ]));
        assert_eq!(chain.routes[0].last_read, 1);
        assert_eq!(chain.routes[1].input, Input::Named(0));
        assert_eq!(chain.routes[1].last_read, 2);
        assert_eq!(chain.routes[2].input, Input::Original);
        assert_eq!(
            chain.routes[2]
                .buffers
                .iter()
                .map(|binding| binding.source.clone())
                .collect::<Vec<_>>(),
            [
                Some(BufferSource::Current(1)),
                None,
                Some(BufferSource::Current(1))
            ]
        );
        assert_eq!(chain.routes[2].last_read, 2);
        assert_eq!(chain.routes[3].input, Input::Previous);
    }
}
