//! Interface shared by the material shader validator and 3D render passes.

pub fn resource_key(path: &str) -> String {
    format!("material-shader:{path}")
}

pub const INTERFACE: &str = r#"
struct XrVertexInput {
    position: vec3<f32>,
    parameters: vec3<f32>,
}

struct XrMaterialInput {
    uv0: vec2<f32>,
    uv1: vec2<f32>,
    world_position: vec3<f32>,
    view_direction: vec3<f32>,
    vertex_color: vec4<f32>,
    parameters: vec3<f32>,
    front_facing: bool,
}

struct XrMaterialSurface {
    base_color: vec3<f32>,
    emissive: vec3<f32>,
    normal: vec3<f32>,
    roughness: f32,
    metallic: f32,
}

const XR_DIRECTIONAL_LIGHT: u32 = 0u;
const XR_POINT_LIGHT: u32 = 1u;
const XR_SPOT_LIGHT: u32 = 2u;

struct XrLightingInput {
    material: XrMaterialInput,
    surface: XrMaterialSurface,
    fresnel_f0: vec3<f32>,
}

struct XrLightInput {
    shading: XrLightingInput,
    kind: u32,
    light_index: u32,
    direction: vec3<f32>,
    color: vec3<f32>,
    attenuation: f32,
    shadow: f32,
    fresnel: vec3<f32>,
}

struct XrLightResponse {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

struct XrAmbientInput {
    shading: XrLightingInput,
    irradiance: vec3<f32>,
    occlusion: f32,
}
"#;

pub const MAX_TEXTURES: usize = 4;

pub const VALIDATION_TEXTURES: &str = r#"
fn material_has_texture(index: u32) -> bool { return false; }
fn material_texture_size(index: u32) -> vec2<u32> { return vec2(0u); }
fn material_texture(index: u32, uv: vec2<f32>) -> vec4<f32> { return vec4(0.0); }
fn material_texture_world_position(index: u32, uv: vec2<f32>) -> XrCapturePosition {
    return xr_invalid_capture_position();
}
"#;

#[derive(Clone, Copy, Default)]
pub struct MaterialHooks {
    pub vertex: bool,
    pub surface: bool,
    pub light: bool,
    pub ambient: bool,
    pub capture_position: bool,
}

impl MaterialHooks {
    const HEADER: &str = "// XRack material hooks: ";

    pub fn from_source(source: &str) -> Self {
        let mask = source
            .strip_prefix(Self::HEADER)
            .and_then(|rest| rest.split_once('\n'))
            .and_then(|(mask, _)| u8::from_str_radix(mask, 16).ok())
            .unwrap_or(0);
        Self {
            vertex: mask & 1 != 0,
            surface: mask & 2 != 0,
            light: mask & 4 != 0,
            ambient: mask & 8 != 0,
            capture_position: mask & 16 != 0,
        }
    }

    pub fn fragment(self) -> bool {
        self.surface || self.light || self.ambient
    }

    pub fn prepare(self, source: &str) -> String {
        let mask = u8::from(self.vertex)
            | (u8::from(self.surface) << 1)
            | (u8::from(self.light) << 2)
            | (u8::from(self.ambient) << 3)
            | (u8::from(self.capture_position) << 4);
        let mut prepared = format!(
            "{}{mask:x}\n{source}\nconst XRACK_VERTEX_PROGRAM: bool = {};\nconst XRACK_DIRECT_LIGHT_PROGRAM: bool = {};\nconst XRACK_AMBIENT_PROGRAM: bool = {};\n",
            Self::HEADER,
            self.vertex,
            self.light,
            self.ambient
        );
        for (present, identity) in [
            (self.vertex, IDENTITY_VERTEX),
            (self.surface, IDENTITY_SURFACE),
            (self.light, IDENTITY_LIGHT),
            (self.ambient, IDENTITY_AMBIENT),
        ] {
            if !present {
                prepared.push_str(identity);
            }
        }
        prepared
    }
}

const IDENTITY_SURFACE: &str = r#"
fn scene_material(input: XrMaterialInput, surface: XrMaterialSurface) -> XrMaterialSurface {
    return surface;
}
"#;

const IDENTITY_LIGHT: &str = r#"
fn scene_light(input: XrLightInput, response: XrLightResponse) -> XrLightResponse {
    return response;
}
"#;

const IDENTITY_AMBIENT: &str = r#"
fn scene_ambient(input: XrAmbientInput, diffuse: vec3<f32>) -> vec3<f32> {
    return diffuse;
}
"#;

const IDENTITY_VERTEX: &str = r#"
fn scene_vertex(input: XrVertexInput) -> vec3<f32> {
    return vec3(0.0);
}
"#;

pub const VERTEX_APPLICATION: &str = r#"
fn xr_displaced_vertex(position: vec3<f32>, parameters: vec4<f32>) -> vec3<f32> {
    if !XRACK_VERTEX_PROGRAM || parameters.w <= 0.0 {
        return position;
    }
    let offset: vec3<f32> = scene_vertex(XrVertexInput(position, parameters.xyz));
    let bound = vec3(parameters.w);
    // Non-finite displacements must not enter clip position.
    let finite_offset = select(vec3(0.0), offset, abs(offset) <= vec3(3.402823e38));
    return position + clamp(finite_offset, -bound, bound);
}
"#;

pub const VALIDATION_VERTEX_ENTRY: &str = r#"
@vertex fn xr_vertex_check(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let position = vec3(f32(index), 0.0, 0.0);
    return vec4(xr_displaced_vertex(position, vec4(1.0)), 1.0);
}
"#;

pub const VALIDATION_ENTRY: &str = r#"
@fragment fn xr_material_check(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let input = XrMaterialInput(at.xy, at.xy, at.xyz, vec3(0.0, 0.0, 1.0),
        vec4(1.0), vec3(0.0), true);
    let surface = XrMaterialSurface(vec3(1.0), vec3(0.0), vec3(0.0, 0.0, 1.0), 0.5, 0.0);
    let result: XrMaterialSurface = scene_material(input, surface);
    let shading = XrLightingInput(input, result, vec3(0.04));
    let light = XrLightInput(shading, XR_DIRECTIONAL_LIGHT, 0u, vec3(0.0, 0.0, 1.0),
        vec3(1.0), 1.0, 1.0, vec3(0.04));
    let response: XrLightResponse = scene_light(light, XrLightResponse(vec3(1.0), vec3(0.0)));
    let ambient: vec3<f32> = scene_ambient(XrAmbientInput(shading, vec3(0.5), 1.0), vec3(0.25));
    return vec4(result.base_color + result.emissive + result.normal + response.diffuse
        + response.specular + ambient, result.roughness + result.metallic);
}
"#;
