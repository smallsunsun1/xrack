override ANISOTROPIC_MATERIAL: bool = false;
override CUSTOM_BASE_FRESNEL: bool = false;
override CUSTOM_COAT_FRESNEL: bool = false;
override CUSTOM_MATERIAL: bool = false;
override CAPTURE_EFFECT_SURFACE: bool = false;
override CAPTURE_LIT_SURFACE: bool = false;
override CAPTURE_MATERIAL_DATA: bool = false;
override CAPTURE_MATERIAL_OCCLUSION: bool = false;
override USE_VERTEX_COLORS: bool = true;
override APPLY_VERTEX_MASKS: bool = false;
override REMAP_MATERIAL_CHANNELS: bool = false;
override NO_DEPTH_EFFECTS: bool = false;
override LIMIT_BLEND_TINT: bool = false;
override TANGENT_VERTEX: bool = false;
override VOLUMETRIC_FOG: bool = false;
override FOG_SCATTERING: bool = false;
override FOG_HEIGHT_PROFILE: bool = false;
override FOG_DENSITY_NOISE: bool = false;
override VOLUME_MATERIAL: bool = false;
override DISPERSION_MATERIAL: bool = false;
override SCREEN_REFLECTIONS: bool = false;
override SCREEN_INDIRECT_LIGHT: bool = false;
override PACK_REFLECTION_ATTRIBUTES: bool = false;
override BAKED_LIGHTMAP: bool = false;
override MODEL_PARTICLES: bool = false;
override REVERSE_DEVICE_DEPTH: bool = false;

struct Globals {
    clip0: vec4<f32>,
    clip1: vec4<f32>,
    clip2: vec4<f32>,
    clip3: vec4<f32>,
    eye: vec4<f32>,
    view_depth: vec4<f32>,
    // Linear fog RGB and overall density.
    fog: vec4<f32>,
    // Near depth, reciprocal depth span, depth curve, transmission curve (0 disables).
    fog_depth: vec4<f32>,
    // Clear world Y, reciprocal signed height span, height curve.
    fog_height: vec4<f32>,
    oit_depth_scale: f32,
    specular_aa: f32,
    background_alpha: f32,
    // 0 for conventional depth, 1 for oblique planes, 2 for an authored matrix.
    oblique_depth: f32,
    // Strength, radius and full-resolution target size.
    ao: vec4<f32>,
    // Lateral clip scales and offsets.
    lens: vec4<f32>,
    // RGB clear color and skybox-enable flag.
    volume_background: vec4<f32>,
    // Near depth, far depth, number of visible volumes, density sample budget.
    volume_clip_count: vec4<f32>,
    // Scattering strength, Henyey-Greenstein asymmetry, and ray sample budget.
    volume_lighting: vec4<f32>,
    volumes: array<FogVolume, 8>,
    camera_right: vec4<f32>,
    camera_up: vec4<f32>,
    custom_viewer: vec4<f32>,
    custom_inverse_clip: array<vec4<f32>, 4>,
}

struct Material {
    base: vec4<f32>,
    // alpha cutoff, imported material, has base texture, alpha mode
    params: vec4<f32>,
    base_uv: TextureTransform,
    channels: vec4<u32>,
    shader_parameters: vec4<f32>,
    // Metallic, roughness, occlusion strength, shading workflow (0 unlit, 1 MR, 2 SG).
    pbr: vec4<f32>,
    // Emissive RGB and normal strength.
    emissive: vec4<f32>,
    // UV slots for metallic-roughness, emissive, occlusion and normal. -1 means absent.
    map_slots: vec4<f32>,
    // RGB specular multiplier, A specular weight.
    specular: vec4<f32>,
    // IOR-based F0, strength texture UV slot, color texture UV slot.
    dielectric: vec4<f32>,
    // Transmission strength, texture UV slot, IOR roughness scale.
    transmission: vec4<f32>,
    // Mesh-space thickness, thickness UV slot, reciprocal IOR.
    volume: vec4<f32>,
    // Red and blue reciprocal IORs, followed by their roughness scales.
    spectral: vec4<f32>,
    // Linear attenuation color and world-space reference distance (0 = infinite).
    absorption: vec4<f32>,
    volume_inverse: array<vec4<f32>, 3>,
    transforms: array<TextureTransform, 8>,
    // Schlick power, scale and bias.
    fresnel: vec4<f32>,
    // Directional roughness strength, rotation cosine/sine, map UV slot.
    anisotropy: vec4<f32>,
    anisotropy_uv: TextureTransform,
    shader_texture_flags: vec4<u32>,
}

struct ClearcoatSettings {
    // Amount, roughness, normal strength.
    factors: vec4<f32>,
    // UV slots for amount, roughness and coat normal. -1 means absent.
    map_slots: vec4<f32>,
    transforms: array<TextureTransform, 3>,
    fresnel: vec4<f32>,
}

struct Light {
    position_kind: vec4<f32>,
    axis_inverse_range: vec4<f32>,
    // RGB radiance and the one-based index of the shadow light.
    radiance: vec4<f32>,
    // Cone scale, cone offset, and fully baked light flag.
    cone: vec4<f32>,
}

struct Lighting {
    ambient: vec4<f32>,
    // Active light count and explicit light list flag.
    settings: vec4<f32>,
    lights: array<Light, 16>,
    // Strength, filter radius in texels, tap count and receiver bias.
    shadow_sampling: array<vec4<f32>, 4>,
}

struct ShadowLight {
    layers: array<vec4<i32>, 2>,
    // Near, far, cascade count (0 for other lights), blend ratio.
    parameters: vec4<f32>,
    cascade_ends: vec4<f32>,
}

struct ShadowProjections {
    rows: array<mat4x4<f32>, 24>,
    maps: array<ShadowLight, 4>,
}

struct Environment {
    // Inverse yaw sine/cosine, intensity and highest valid roughness mip.
    // A negative mip count selects the probe-free path.
    settings: vec4<f32>,
    diffuse: array<vec4<f32>, 9>,
    specular: vec4<f32>,
    // Probe capture position and box projection flag.
    capture: vec4<f32>,
    box_min: vec4<f32>,
    box_max: vec4<f32>,
}

@group(2) @binding(0) var<uniform> environment: Environment;
@group(2) @binding(1) var probe: texture_cube<f32>;
@group(2) @binding(2) var probe_sampler: sampler;

@group(0) @binding(2) var<uniform> shadows: ShadowProjections;
@group(0) @binding(3) var shadow_depth: texture_depth_2d_array;
@group(0) @binding(4) var shadow_sampler: sampler_comparison;
struct OcclusionSamples {
    size: vec2<u32>,
    padding: vec2<u32>,
    samples: array<vec2<f32>>,
}
@group(0) @binding(5) var<storage, read> ambient_occlusion: OcclusionSamples;

@group(0) @binding(6) var opaque_radiance: texture_2d<f32>;
@group(0) @binding(7) var transmission_sampler: sampler;
@group(0) @binding(8) var<storage, read> opaque_depth: array<f32>;

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<uniform> lighting: Lighting;
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var base_sampler: sampler;
@group(1) @binding(3) var mr_texture: texture_2d<f32>;
@group(1) @binding(4) var mr_sampler: sampler;
@group(1) @binding(5) var emissive_texture: texture_2d<f32>;
@group(1) @binding(6) var emissive_sampler: sampler;
@group(1) @binding(7) var occlusion_texture: texture_2d<f32>;
@group(1) @binding(8) var occlusion_sampler: sampler;
@group(1) @binding(9) var normal_texture: texture_2d<f32>;
@group(1) @binding(10) var normal_sampler: sampler;
@group(1) @binding(11) var specular_texture: texture_2d<f32>;
@group(1) @binding(12) var specular_sampler: sampler;
@group(1) @binding(13) var specular_color_texture: texture_2d<f32>;
@group(1) @binding(14) var specular_color_sampler: sampler;
@group(1) @binding(15) var transmission_texture: texture_2d<f32>;
@group(1) @binding(16) var transmission_map_sampler: sampler;
@group(1) @binding(17) var thickness_texture: texture_2d<f32>;
@group(1) @binding(18) var thickness_sampler: sampler;
@group(1) @binding(19) var anisotropy_texture: texture_2d<f32>;
@group(1) @binding(20) var anisotropy_sampler: sampler;
@group(1) @binding(21) var coat_amount_texture: texture_2d<f32>;
@group(1) @binding(22) var coat_amount_sampler: sampler;
@group(1) @binding(23) var coat_roughness_texture: texture_2d<f32>;
@group(1) @binding(24) var coat_roughness_sampler: sampler;
@group(1) @binding(25) var coat_normal_texture: texture_2d<f32>;
@group(1) @binding(26) var coat_normal_sampler: sampler;
@group(1) @binding(27) var<uniform> clearcoat: ClearcoatSettings;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) instance_metadata: vec2<u32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) uv1: vec2<f32>,
    @location(6) world: vec3<f32>,
    @location(7) tangent: vec3<f32>,
    @location(9) @interpolate(flat) model_x: vec3<f32>,
    @location(10) @interpolate(flat) model_y: vec3<f32>,
    @location(11) @interpolate(flat) model_z: vec3<f32>,
    @location(12) lightmap_uv: vec2<f32>,
    @location(13) @interpolate(flat) lightmap_atlas: vec4<f32>,
    @location(14) @interpolate(flat) lightmap_options: vec2<f32>,
    @location(15) @interpolate(flat) instance_data: vec4<f32>,
    // XRack wireframe varying
}

struct VertexInput {
    @builtin(vertex_index) index: u32,
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) model0: vec4<f32>,
    @location(5) model1: vec4<f32>,
    @location(6) model2: vec4<f32>,
    @location(7) tint: vec4<f32>,
    @location(8) metadata: u32,
    @location(9) uv1: vec2<f32>,
    @location(15) instance_data: vec4<f32>,
}

@vertex
fn vertex_main(vertex: VertexInput) -> VertexOut {
    return transform_vertex(vertex, vec4<f32>(0.0));
}

@vertex
fn vertex_tangent(vertex: VertexInput, @location(10) tangent: vec4<f32>) -> VertexOut {
    return transform_vertex(vertex, tangent);
}

fn lightmapped(vertex: VertexOut, uv: vec2<f32>, instance: u32) -> VertexOut {
    // The transparent passes bind captured depth here. The baked opaque
    // variant binds the matching instance parameter stream instead.
    let i = instance * 8u;
    var result = vertex;
    result.lightmap_uv = uv;
    result.lightmap_atlas = vec4<f32>(opaque_depth[i], opaque_depth[i + 1u], opaque_depth[i + 2u], opaque_depth[i + 3u]);
    result.lightmap_options = vec2<f32>(opaque_depth[i + 4u], opaque_depth[i + 5u]);
    return result;
}

@vertex
fn vertex_lightmapped(vertex: VertexInput, @location(11) uv: vec2<f32>,
    @builtin(instance_index) instance: u32) -> VertexOut {
    return lightmapped(transform_vertex(vertex, vec4<f32>(0.0)), uv, instance);
}

@vertex
fn vertex_lightmapped_tangent(vertex: VertexInput, @location(10) tangent: vec4<f32>,
    @location(11) uv: vec2<f32>, @builtin(instance_index) instance: u32) -> VertexOut {
    return lightmapped(transform_vertex(vertex, tangent), uv, instance);
}

@vertex
fn vertex_skin(vertex: VertexInput, weights: SkinWeights) -> VertexOut {
    return transform_skinned_vertex(vertex, weights, vec4<f32>(0.0));
}

@vertex
fn vertex_skin_tangent(vertex: VertexInput, weights: SkinWeights, @location(10) tangent: vec4<f32>) -> VertexOut {
    return transform_skinned_vertex(vertex, weights, tangent);
}

fn transform_skinned_vertex(vertex: VertexInput, weights: SkinWeights, tangent: vec4<f32>) -> VertexOut {
    let skin = skin_transform(weights);
    let linear = skin_columns(skin);
    var deformed = vertex;
    deformed.position = skin_point(skin, vertex.position);
    deformed.normal = skin_normal(linear, vertex.normal);
    let sign = tangent.w * select(1.0, -1.0, determinant(linear) < 0.0);
    return transform_vertex(deformed, vec4<f32>(linear * tangent.xyz, sign));
}

fn morphed_vertex(vertex: VertexInput, delta: MorphDisplacement) -> VertexInput {
    var deformed = vertex;
    deformed.position += delta.position;
    deformed.normal = morph_normal(vertex.normal, delta.normal);
    return deformed;
}

@vertex
fn vertex_morph(vertex: VertexInput) -> VertexOut {
    let delta = morph_displacement(vertex.index);
    return transform_vertex(morphed_vertex(vertex, delta), vec4<f32>(0.0));
}

@vertex
fn vertex_morph_tangent(vertex: VertexInput,
                        @location(10) tangent: vec4<f32>) -> VertexOut {
    let delta = morph_displacement(vertex.index);
    return transform_vertex(morphed_vertex(vertex, delta), vec4<f32>(tangent.xyz + delta.tangent, tangent.w));
}

@vertex
fn vertex_skin_morph(vertex: VertexInput, weights: SkinWeights) -> VertexOut {
    let delta = morph_displacement(vertex.index);
    return transform_skinned_vertex(morphed_vertex(vertex, delta), weights, vec4<f32>(0.0));
}

@vertex
fn vertex_skin_morph_tangent(vertex: VertexInput, weights: SkinWeights,
                             @location(10) tangent: vec4<f32>) -> VertexOut {
    let delta = morph_displacement(vertex.index);
    return transform_skinned_vertex(morphed_vertex(vertex, delta), weights, vec4<f32>(tangent.xyz + delta.tangent, tangent.w));
}

fn transform_vertex(vertex: VertexInput, tangent: vec4<f32>) -> VertexOut {
    let local = vec4(xr_displaced_vertex(vertex.position, material.shader_parameters, vertex.instance_data,
        vertex.metadata >> 3u, vertex.tint, mat3x4(vertex.model0, vertex.model1, vertex.model2)), 1.0);
    let world = vec4<f32>(
        dot(vertex.model0, local), dot(vertex.model1, local), dot(vertex.model2, local), 1.0,
    );
    let clip = vec4<f32>(
        dot(globals.clip0, world),
        dot(globals.clip1, world),
        dot(globals.clip2, world),
        dot(globals.clip3, world),
    );
    let x = vec3<f32>(vertex.model0.x, vertex.model1.x, vertex.model2.x);
    let y = vec3<f32>(vertex.model0.y, vertex.model1.y, vertex.model2.y);
    let z = vec3<f32>(vertex.model0.z, vertex.model1.z, vertex.model2.z);
    // Cofactors include the determinant sign. Remove it for inverse-transpose
    // normals, and apply it to the tangent frame's handedness.
    let orientation = select(1.0, -1.0, (vertex.metadata & 4u) != 0u);
    let normal = orientation * (vertex.normal.x * cross(y, z)
        + vertex.normal.y * cross(z, x)
        + vertex.normal.z * cross(x, y));
    let world_tangent = x * tangent.x + y * tangent.y + z * tangent.z;
    return VertexOut(
        clip, normal, vertex.tint, vec2(vertex.metadata, bitcast<u32>(tangent.w * orientation)), vertex.uv, vertex.color,
        vertex.uv1, world.xyz, world_tangent,
        select(vec3<f32>(0.0), x, VOLUME_MATERIAL || ANISOTROPIC_MATERIAL),
        select(vec3<f32>(0.0), y, VOLUME_MATERIAL || ANISOTROPIC_MATERIAL),
        select(vec3<f32>(0.0), z, VOLUME_MATERIAL || ANISOTROPIC_MATERIAL),
        vec2<f32>(-1.0), vec4<f32>(0.0), vec2<f32>(0.0), vertex.instance_data,
        // XRack wireframe corner
    );
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return select(color / 12.92, pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), color > vec3<f32>(0.04045));
}

fn linear_to_srgb(color: vec3<f32>) -> vec3<f32> {
    return select(color * 12.92, 1.055 * pow(color, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055), color > vec3<f32>(0.0031308));
}

struct UvDerivatives {
    dx0: vec2<f32>,
    dy0: vec2<f32>,
    dx1: vec2<f32>,
    dy1: vec2<f32>,
}

fn sample_material_map(
    map: texture_2d<f32>, filtering: sampler, vertex: VertexOut, uv: UvDerivatives, slot: f32,
    transform: TextureTransform,
) -> vec4<f32> {
    let second = slot > 0.5;
    return textureSampleGrad(
        map, filtering, texture_uv(transform, select(vertex.uv, vertex.uv1, second)),
        texture_gradient(transform, select(uv.dx0, uv.dx1, second)),
        texture_gradient(transform, select(uv.dy0, uv.dy1, second)),
    );
}

fn material_channel(role: u32) -> u32 {
    return (material.channels[role / 8u] >> (4u * (role % 8u))) & 15u;
}

fn scalar_texel(texel: vec4<f32>, role: u32, fallback: u32) -> f32 {
    if REMAP_MATERIAL_CHANNELS {
        return texel[material_channel(role)];
    }
    return texel[fallback];
}

fn color_texel(texel: vec4<f32>, role: u32) -> vec4<f32> {
    if REMAP_MATERIAL_CHANNELS {
        let channel = material_channel(role);
        if channel < 4u {
            return vec4<f32>(vec3<f32>(texel[channel]), 1.0);
        }
    }
    return texel;
}

const VERTEX_METALNESS: u32 = 0u;
const VERTEX_ROUGHNESS: u32 = 1u;
const VERTEX_OCCLUSION: u32 = 2u;
const VERTEX_SPECULAR: u32 = 3u;
const VERTEX_NORMAL: u32 = 4u;
const VERTEX_CLEARCOAT: u32 = 5u;
const VERTEX_CLEARCOAT_ROUGHNESS: u32 = 6u;
const VERTEX_CLEARCOAT_NORMAL: u32 = 7u;
const VERTEX_TRANSMISSION: u32 = 8u;
const VERTEX_THICKNESS: u32 = 9u;

fn vertex_mask(color: vec4<f32>, role: u32) -> f32 {
    if APPLY_VERTEX_MASKS {
        let channel = (material.channels.z >> (3u * role)) & 7u;
        if channel >= 2u {
            return clamp(color[channel - 2u], 0.0, 1.0);
        }
    }
    return 1.0;
}

struct TangentFrame {
    tangent: vec3<f32>,
    sign: f32,
}

fn mapped_normal(
    vertex: VertexOut, n: vec3<f32>, uv: UvDerivatives, frame: TangentFrame,
    map: texture_2d<f32>, filtering: sampler, slot: f32, scale: f32, transform: TextureTransform,
) -> vec3<f32> {
    if slot < 0.0 || abs(frame.sign) < 0.5 || scale == 0.0 {
        return n;
    }
    let tangent = frame.tangent - n * dot(frame.tangent, n);
    if dot(tangent, tangent) < 0.00000001 {
        return n;
    }
    let t = normalize(tangent);
    let b = cross(n, t) * frame.sign;
    let sample = sample_material_map(map, filtering, vertex, uv, slot, transform).rgb;
    let local = sample * 2.0 - vec3<f32>(1.0);
    let scaled = vec3<f32>(local.xy * scale, local.z);
    // Rescale before normalizing so a large but finite strength cannot overflow the dot product.
    let maximum = max(abs(scaled.x), max(abs(scaled.y), abs(scaled.z)));
    if maximum < 0.00001 {
        return n;
    }
    let direction = scaled / maximum;
    return normalize(t * direction.x + b * direction.y + n * direction.z);
}

fn alternate_tangent_frame(vertex: VertexOut, n: vec3<f32>, uv: UvDerivatives, slot: f32, transform: TextureTransform) -> TangentFrame {
    let px = dpdx(vertex.world);
    let py = dpdy(vertex.world);
    let dx = texture_gradient(transform, select(uv.dx0, uv.dx1, slot > 0.5));
    let dy = texture_gradient(transform, select(uv.dy0, uv.dy1, slot > 0.5));
    let determinant = dx.x * dy.y - dx.y * dy.x;
    if abs(determinant) < 0.000000000001 {
        return TangentFrame(vec3<f32>(0.0), 0.0);
    }
    let tangent = (px * dy.y - py * dx.y) * sign(determinant);
    let bitangent = (py * dx.x - px * dy.x) * sign(determinant);
    if dot(tangent, tangent) < 0.000000000001 {
        return TangentFrame(vec3<f32>(0.0), 0.0);
    }
    let handedness = sign(dot(cross(n, tangent), bitangent));
    return TangentFrame(normalize(tangent), handedness);
}

struct AnisotropicLobe {
    axis: vec3<f32>,
    across: vec3<f32>,
    strength: f32,
}

fn brush_direction(vertex: VertexOut, n: vec3<f32>, uv: UvDerivatives, frame: TangentFrame) -> AnisotropicLobe {
    var tangent = frame.tangent - n * dot(frame.tangent, n);
    var handedness = frame.sign;
    if dot(tangent, tangent) <= 1e-8 || abs(handedness) < 0.5 {
        // A model-space direction keeps an unparameterized mesh stable as the camera moves.
        let axis_x = vertex.model_x - n * dot(vertex.model_x, n);
        let axis_y = vertex.model_y - n * dot(vertex.model_y, n);
        tangent = select(axis_y, axis_x, dot(axis_x, axis_x) > 1e-8);
        handedness = select(1.0, -1.0, dot(cross(vertex.model_x, vertex.model_y), vertex.model_z) < 0.0);
    }
    if dot(tangent, tangent) <= 1e-12 {
        let axis = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.x) < 0.75);
        tangent = axis - n * dot(axis, n);
    }
    let t = normalize(tangent);
    let b = cross(n, t) * handedness;
    var direction = vec2<f32>(1.0, 0.0);
    var strength = material.anisotropy.x;
    if material.anisotropy.w >= 0.0 {
        let texel = sample_material_map(anisotropy_texture, anisotropy_sampler, vertex, uv,
            material.anisotropy.w, material.anisotropy_uv);
        let at = texel.rg * 2.0 - 1.0;
        if dot(at, at) > 1e-8 {
            direction = normalize(at);
        }
        strength *= texel.b;
    }
    let rotation = material.anisotropy.yz;
    direction = vec2<f32>(
        direction.x * rotation.x - direction.y * rotation.y,
        direction.x * rotation.y + direction.y * rotation.x,
    );
    let axis = normalize(t * direction.x + b * direction.y);
    return AnisotropicLobe(axis, cross(n, axis), strength);
}

fn anisotropic_scale(lobe: AnisotropicLobe, l: vec3<f32>, v: vec3<f32>, h: vec3<f32>,
    nl: f32, nv: f32, nh: f32, roughness_squared: f32, smith_v: f32) -> f32 {
    if lobe.strength <= 0.0 {
        return ggx_scale(nl, nv, nh, roughness_squared * roughness_squared, smith_v);
    }
    let ab = roughness_squared;
    let at = mix(ab, 1.0, lobe.strength * lobe.strength);
    // Anisotropic GGX distribution and height-correlated Smith masking.
    let half_vector = vec3<f32>(dot(lobe.axis, h) / at, dot(lobe.across, h) / ab, nh);
    let denominator = max(dot(half_vector, half_vector), 1e-6);
    let distribution = 1.0 / (3.14159265 * at * ab * denominator * denominator);
    let view_mask = length(vec3<f32>(at * dot(lobe.axis, v), ab * dot(lobe.across, v), nv));
    let light_mask = length(vec3<f32>(at * dot(lobe.axis, l), ab * dot(lobe.across, l), nl));
    return distribution * 0.5 / max(nl * view_mask + nv * light_mask, 0.00001);
}

fn anisotropic_probe_normal(n: vec3<f32>, v: vec3<f32>, lobe: AnisotropicLobe, roughness: f32) -> vec3<f32> {
    let projected = v - lobe.across * dot(v, lobe.across);
    if lobe.strength <= 0.0 || dot(projected, projected) <= 1e-8 {
        return n;
    }
    let amount = 1.0 - lobe.strength * (1.0 - roughness);
    let squared = amount * amount;
    return normalize(mix(normalize(projected), n, squared * squared));
}

fn specular_kernel(normal: vec3<f32>) -> f32 {
    // Approximate the normal distribution over a pixel in projected space.
    // Tokuyoshi and Kaplanyan (JCGT 2021), Eq. 14.
    let dx = dpdx(normal);
    let dy = dpdy(normal);
    return min((dot(dx, dx) + dot(dy, dy)) * 0.15915494, 0.18);
}

fn filtered_roughness(roughness: f32, kernel: f32) -> f32 {
    if kernel <= 0.0 {
        return roughness;
    }
    // Convert glTF perceptual roughness to the squared GGX parameter.
    let alpha = roughness * roughness;
    return sqrt(sqrt(min(alpha * alpha + kernel, 1.0)));
}

struct CoatSurface {
    normal: vec3<f32>,
    amount: f32,
    roughness: f32,
    fresnel: vec4<f32>,
}

fn coat_surface(vertex: VertexOut, uv: UvDerivatives, front_facing: bool) -> CoatSurface {
    var amount = clearcoat.factors.x * vertex_mask(vertex.color, VERTEX_CLEARCOAT);
    if clearcoat.map_slots.x >= 0.0 {
        let texel = sample_material_map(coat_amount_texture, coat_amount_sampler, vertex, uv, clearcoat.map_slots.x, clearcoat.transforms[0]);
        amount *= scalar_texel(texel, 9u, 0u);
    }
    var roughness = clearcoat.factors.y * vertex_mask(vertex.color, VERTEX_CLEARCOAT_ROUGHNESS);
    if clearcoat.map_slots.y >= 0.0 {
        let texel = sample_material_map(coat_roughness_texture, coat_roughness_sampler, vertex, uv, clearcoat.map_slots.y, clearcoat.transforms[1]);
        roughness *= scalar_texel(texel, 10u, 1u);
    }
    let normal = normalize(vertex.normal);
    let slot = clearcoat.map_slots.z;
    var frame = TangentFrame(vertex.tangent, bitcast<f32>(vertex.instance_metadata.y));
    let transform = clearcoat.transforms[2];
    let base_transform = material.transforms[3];
    // Offsets affect the lookup only. Different derivatives need their own frame.
    if slot >= 0.0 && (!TANGENT_VERTEX || material.map_slots.w >= 0.0 &&
        (slot != material.map_slots.w || any(transform.u.xy != base_transform.u.xy) ||
            any(transform.v.xy != base_transform.v.xy))) {
        frame = alternate_tangent_frame(vertex, normal, uv, slot, transform);
    }
    let mapped = mapped_normal(vertex, normal, uv, frame, coat_normal_texture,
                               coat_normal_sampler, slot,
                               clearcoat.factors.z * vertex_mask(vertex.color, VERTEX_CLEARCOAT_NORMAL), transform);
    if globals.specular_aa > 0.5 && clearcoat.factors.x > 0.0 {
        roughness = filtered_roughness(roughness, specular_kernel(mapped));
    }
    return CoatSurface(select(-mapped, mapped, front_facing), amount, roughness, clearcoat.fresnel);
}

struct IncidentLight {
    axis: vec3<f32>,
    radiance: vec3<f32>,
    attenuation: f32,
}

fn incident_light(light: Light, world: vec3<f32>) -> IncidentLight {
    let kind = light.position_kind.w;
    if kind == 0.0 {
        return IncidentLight(light.axis_inverse_range.xyz, light.radiance.xyz, 1.0);
    }
    let delta = light.position_kind.xyz - world;
    let distance_squared = dot(delta, delta);
    let none = IncidentLight(vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    if distance_squared <= 0.000001 {
        return none;
    }
    var fade = 1.0;
    if light.axis_inverse_range.w > 0.0 {
        let fraction = distance_squared * light.axis_inverse_range.w;
        if fraction >= 1.0 {
            return none;
        }
        fade = 1.0 - fraction * fraction;
    }
    let axis = delta * inverseSqrt(distance_squared);
    if kind == 2.0 {
        let cosine = dot(axis, light.axis_inverse_range.xyz);
        let cone = clamp(cosine * light.cone.x + light.cone.y, 0.0, 1.0);
        if cone == 0.0 {
            return none;
        }
        fade *= cone * cone;
    }
    let attenuation = fade / max(distance_squared, 0.0001);
    return IncidentLight(axis, min(light.radiance.xyz * attenuation, vec3<f32>(1000000.0)), attenuation);
}

// This is the same axis and sign order used by PointShadowProjection.
fn point_shadow_face(direction: vec3<f32>) -> u32 {
    let major = abs(direction);
    if major.x >= max(major.y, major.z) {
        return select(0u, 1u, direction.x < 0.0);
    }
    if major.y >= major.z {
        return select(2u, 3u, direction.y < 0.0);
    }
    return select(4u, 5u, direction.z < 0.0);
}

struct ShadowCoordinate {
    uv: vec2<f32>,
    reference: f32,
    layer: i32,
}

const NO_SHADOW = ShadowCoordinate(vec2(0.0), 0.0, -1);
const SHADOW_OFFSETS = array<vec2<f32>, 16>(
    vec2(-0.5, -0.5), vec2(0.5, -0.5), vec2(0.5, 0.5), vec2(-0.5, 0.5),
    vec2(-1.0, 0.0), vec2(0.0, -1.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
    vec2(-0.92, -0.38), vec2(-0.38, -0.92), vec2(0.38, -0.92), vec2(0.92, -0.38),
    vec2(0.92, 0.38), vec2(0.38, 0.92), vec2(-0.38, 0.92), vec2(-0.92, 0.38),
);

fn shadow_coordinate(map: ShadowLight, world: vec3<f32>, face: u32, point: bool, bias: f32) -> ShadowCoordinate {
    let layer = map.layers[face / 4u][face % 4u] - 1;
    if layer < 0 {
        return NO_SHADOW;
    }
    // The matrix is encoded as rows. A row vector times its columns takes
    // the dot products with these four rows in the original projection.
    let clip = vec4<f32>(world, 1.0) * shadows.rows[layer];
    if clip.w <= 0.0 {
        return NO_SHADOW;
    }
    let ndc = clip.xyz / clip.w;
    if any(abs(ndc.xy) > vec2<f32>(1.0)) || ndc.z < 0.0 || ndc.z > 1.0 {
        return NO_SHADOW;
    }
    let uv = ndc.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    var reference = ndc.z - bias;
    if point {
        // Bias the world-space receiver depth before the perspective divide.
        // A constant normalized depth subtraction would erase distant shadows.
        let near = map.parameters.x;
        let far = map.parameters.y;
        let biased_depth = clip.w - bias * far;
        if biased_depth <= near {
            return NO_SHADOW;
        }
        reference = (far / (far - near)) * (1.0 - near / biased_depth);
    }
    return ShadowCoordinate(uv, reference, layer);
}

fn shadow_compare(coordinate: ShadowCoordinate, hard: bool) -> f32 {
    if coordinate.layer < 0 {
        return 1.0;
    }
    if hard {
        let size = vec2<i32>(textureDimensions(shadow_depth));
        let pixel = clamp(vec2<i32>(coordinate.uv * vec2<f32>(size)), vec2(0), size - 1);
        return f32(coordinate.reference <= textureLoad(shadow_depth, pixel, coordinate.layer, 0));
    }
    return textureSampleCompareLevel(shadow_depth, shadow_sampler,
        coordinate.uv, coordinate.layer, coordinate.reference);
}

fn sample_planar_shadow(map: ShadowLight, sampling: vec4<f32>, world: vec3<f32>, face: u32) -> f32 {
    let coordinate = shadow_coordinate(map, world, face, false, sampling.w);
    let taps = u32(sampling.z);
    if coordinate.layer < 0 || taps <= 1u || sampling.y == 0.0 {
        return shadow_compare(coordinate, taps == 0u);
    }
    let radius = vec2<f32>(sampling.y) / vec2<f32>(textureDimensions(shadow_depth));
    var sum = 0.0;
    for (var i = 0u; i < taps; i += 1u) {
        sum += textureSampleCompareLevel(shadow_depth, shadow_sampler,
            coordinate.uv + radius * SHADOW_OFFSETS[i], coordinate.layer, coordinate.reference);
    }
    return sum / f32(taps);
}

fn sample_point_shadow(map: ShadowLight, sampling: vec4<f32>, world: vec3<f32>, origin: vec3<f32>) -> f32 {
    let delta = world - origin;
    let taps = u32(sampling.z);
    if taps <= 1u || sampling.y == 0.0 {
        return shadow_compare(shadow_coordinate(map, world, point_shadow_face(delta), true, sampling.w), taps == 0u);
    }
    let distance = length(delta);
    if distance <= map.parameters.x || distance > map.parameters.y * 1.732051 {
        return 1.0;
    }
    let axis = delta / distance;
    let seed = select(vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0), abs(axis.z) > 0.95);
    let tangent = normalize(cross(seed, axis));
    let bitangent = cross(axis, tangent);
    let radius = 2.0 * sampling.y / f32(textureDimensions(shadow_depth).x);
    // Select the face for each tap. A kernel can span a cube edge even if
    // the center face has no casters and was omitted from the atlas.
    var sum = 0.0;
    for (var i = 0u; i < taps; i += 1u) {
        let offset = SHADOW_OFFSETS[i] * radius;
        let ray = axis + tangent * offset.x + bitangent * offset.y;
        let sample_world = origin + normalize(ray) * distance;
        sum += shadow_compare(shadow_coordinate(map, sample_world,
            point_shadow_face(ray), true, sampling.w), false);
    }
    return sum / f32(taps);
}

fn cascade_visibility(map: ShadowLight, sampling: vec4<f32>, world: vec3<f32>, count: u32) -> f32 {
    let depth = dot(globals.view_depth, vec4<f32>(world, 1.0));
    if depth < map.parameters.x || depth > map.parameters.y {
        return 1.0;
    }
    var start = map.parameters.x;
    for (var cascade = 0u; cascade + 1u < count; cascade += 1u) {
        let end = map.cascade_ends[cascade];
        let width = 0.5 * map.parameters.w * min(end - start, map.cascade_ends[cascade + 1u] - end);
        if depth < end - width {
            return sample_planar_shadow(map, sampling, world, cascade);
        }
        if width > 0.0 && depth <= end + width {
            let a = sample_planar_shadow(map, sampling, world, cascade);
            let b = sample_planar_shadow(map, sampling, world, cascade + 1u);
            return mix(a, b, smoothstep(end - width, end + width, depth));
        }
        start = end;
    }
    return sample_planar_shadow(map, sampling, world, count - 1u);
}

fn world_shadow_visibility(light: Light, world: vec3<f32>) -> f32 {
    if light.radiance.w < 0.5 {
        return 1.0;
    }
    let slot = i32(light.radiance.w) - 1;
    let sampling = lighting.shadow_sampling[slot];
    if sampling.x <= 0.0 {
        return 1.0;
    }
    let map = shadows.maps[slot];
    var visibility: f32;
    if map.parameters.z > 1.0 {
        visibility = cascade_visibility(map, sampling, world, u32(map.parameters.z));
    } else if light.position_kind.w == 1.0 {
        visibility = sample_point_shadow(map, sampling, world, light.position_kind.xyz);
    } else {
        visibility = sample_planar_shadow(map, sampling, world, 0u);
    }
    return mix(1.0, visibility, sampling.x);
}

fn shadow_visibility(light: Light, vertex: VertexOut) -> f32 {
    if (vertex.instance_metadata.x & 2u) == 0u {
        return 1.0;
    }
    return world_shadow_visibility(light, vertex.world);
}

fn probe_dir(direction: vec3<f32>, rotation: vec2<f32>) -> vec3<f32> {
    let sn = rotation.x;
    let cs = rotation.y;
    return vec3<f32>(cs * direction.x - sn * direction.z, direction.y,
                     sn * direction.x + cs * direction.z);
}

fn environment_diffuse(normal: vec3<f32>) -> vec3<f32> {
    if environment.settings.w < 0.0 {
        return vec3<f32>(0.0);
    }
    let n = probe_dir(normal, environment.settings.xy);
    let c = environment.diffuse;
    let irradiance = 0.282095 * c[0].rgb
        + 0.488603 * (n.y * c[1].rgb + n.z * c[2].rgb + n.x * c[3].rgb)
        + 1.092548 * (n.x * n.y * c[4].rgb + n.y * n.z * c[5].rgb + n.x * n.z * c[7].rgb)
        + 0.315392 * (3.0 * n.z * n.z - 1.0) * c[6].rgb
        + 0.546274 * (n.x * n.x - n.y * n.y) * c[8].rgb;
    return max(vec3<f32>(0.0), irradiance) * environment.settings.z;
}

fn box_reflection(world: vec3<f32>, direction: vec3<f32>) -> vec3<f32> {
    if environment.capture.w < 0.5 || any(world < environment.box_min.xyz) || any(world > environment.box_max.xyz) {
        return direction;
    }
    let face = select(environment.box_min.xyz, environment.box_max.xyz, direction >= vec3<f32>(0.0));
    let nonparallel = abs(direction) > vec3<f32>(1e-6);
    let axes = select(vec3<f32>(1.0), direction, nonparallel);
    let distance = select(vec3<f32>(1e30), (face - world) / axes, nonparallel);
    let endpoint = world + direction * min(distance.x, min(distance.y, distance.z));
    let adjusted = endpoint - environment.capture.xyz;
    return select(direction, adjusted, dot(adjusted, adjusted) > 1e-12);
}

fn environment_specular(world: vec3<f32>, direction: vec3<f32>, f0: vec3<f32>, f90: f32,
    nv: f32, roughness: f32, curve: vec4<f32>, custom: bool) -> vec3<f32> {
    if environment.specular.w < 0.0 {
        return vec3<f32>(0.0);
    }
    let lookup = probe_dir(box_reflection(world, direction), environment.specular.xy);
    let radiance = textureSampleLevel(probe, probe_sampler, lookup, roughness * environment.specular.w).rgb;
    return radiance * reflection_response(f0, f90, nv, roughness, curve, custom) * environment.specular.z;
}

fn reflection_response(f0: vec3<f32>, f90: f32, nv: f32, roughness: f32,
    curve: vec4<f32>, custom: bool) -> vec3<f32> {
    // Analytic fit of the integrated GGX environment BRDF (Lazarov).
    let fit = roughness * vec4<f32>(-1.0, -0.0275, -0.572, 0.022)
        + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    var cosine = nv;
    if custom && curve.x != 5.0 {
        // The integrated fit assumes the fifth-power curve. Reparameterize
        // its angle so the Schlick edge term follows the authored exponent.
        cosine = 1.0 - pow(1.0 - nv, curve.x * 0.2);
    }
    let p = min(fit.x * fit.x, exp2(-9.28 * cosine)) * fit.x + fit.y;
    let ab = vec2<f32>(-1.04, 1.04) * p + fit.zw;
    let response = f0 * ab.x + f90 * ab.y;
    if custom {
        return clamp(response * curve.y + vec3<f32>(curve.z * (ab.x + ab.y)), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return max(vec3<f32>(0.0), response);
}

struct ReflectionData {
    normal_roughness: vec4<f32>,
    response: vec3<f32>,
    probe: vec3<f32>,
}

var<private> screen_reflection: ReflectionData;

struct DiffuseScreenData {
    normal: vec3<f32>,
    response: vec3<f32>,
    ambient: vec3<f32>,
}

var<private> screen_diffuse: DiffuseScreenData;

fn view_normal(normal: vec3<f32>) -> vec3<f32> {
    let forward = globals.view_depth.xyz;
    let right = globals.camera_right.xyz;
    let up = globals.camera_up.xyz;
    return normalize(vec3<f32>(dot(normal, right), dot(normal, up), dot(normal, forward)));
}

fn record_reflection(normal: vec3<f32>, roughness: f32, response: vec3<f32>, probe_color: vec3<f32>) {
    screen_reflection = ReflectionData(vec4<f32>(view_normal(normal), roughness), response, probe_color);
}

fn record_diffuse(normal: vec3<f32>, response: vec3<f32>, ambient: vec3<f32>) {
    screen_diffuse = DiffuseScreenData(view_normal(normal), response, ambient);
}

struct SkyVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn sky_vertex(@builtin(vertex_index) index: u32) -> SkyVertex {
    let ndc = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    return SkyVertex(vec4<f32>(ndc, select(1.0, 0.0, REVERSE_DEVICE_DEPTH), 1.0), ndc);
}

fn custom_forward(world: vec3<f32>) -> vec3<f32> {
    let viewer = globals.custom_viewer;
    if viewer.w == 0.0 {
        return viewer.xyz;
    }
    return normalize((world - viewer.xyz) * viewer.w);
}

fn sky_color(vertex: SkyVertex) -> vec4<f32> {
    let forward = globals.view_depth.xyz;
    let lens = globals.lens;
    let plane = (vertex.ndc - lens.zw) / lens.xy;
    let orthographic = globals.eye.w > 0.5;
    let shift = select(lens.zw, vec2<f32>(0.0), orthographic);
    let right = (globals.clip0.xyz - shift.x * forward) / lens.x;
    let up = (globals.clip1.xyz - shift.y * forward) / lens.y;
    let offset = plane.x * right + plane.y * up;
    var origin = globals.eye.xyz;
    var direction = forward;
    if orthographic {
        origin += offset;
    } else {
        direction = normalize(forward + offset);
    }
    if globals.oblique_depth > 1.5 {
        let p = vec4<f32>(vertex.ndc, 0.0, 1.0);
        let inverse = globals.custom_inverse_clip;
        origin = vec3<f32>(dot(inverse[0], p), dot(inverse[1], p), dot(inverse[2], p))
            / dot(inverse[3], p);
        direction = custom_forward(origin);
    }
    var alpha = globals.background_alpha;
    var color = globals.volume_background.rgb * alpha;
    if !VOLUMETRIC_FOG || globals.volume_background.w > 0.5 {
        color = textureSampleLevel(probe, probe_sampler, probe_dir(direction, environment.settings.xy), 0.0).rgb
            * environment.settings.z;
        alpha = 1.0;
    }
    if VOLUMETRIC_FOG {
        let fog = fog_to_distance(FogRay(origin, direction), 3.402823e38);
        color = color * fog.transmission + fog.radiance;
        alpha = 1.0 - (1.0 - alpha) * fog.transmission;
    }
    return vec4<f32>(min(color, vec3<f32>(65000.0)), alpha);
}

@fragment
fn sky_fragment(vertex: SkyVertex) -> @location(0) vec4<f32> {
    return sky_color(vertex);
}

struct ReflectedColor {
    @location(0) color: vec4<f32>,
    @location(1) normal_roughness: vec4<f32>,
    @location(2) response: vec4<f32>,
    @location(3) probe: vec4<f32>,
}

@fragment
fn sky_with_surfaces(vertex: SkyVertex) -> ReflectedColor {
    let zero = vec4<f32>(0.0);
    return ReflectedColor(sky_color(vertex), zero, zero, zero);
}

struct IndirectColor {
    @location(0) color: vec4<f32>,
    @location(1) surfaces: vec4<u32>,
    @location(2) factors: vec4<u32>,
    @location(3) probe: vec4<f32>,
}

@fragment
fn sky_with_indirect(vertex: SkyVertex) -> IndirectColor {
    let zero = vec4<u32>(0u);
    return IndirectColor(sky_color(vertex), zero, zero, vec4<f32>(0.0));
}

fn fresnel_edge(cosine: f32) -> f32 {
    let edge = 1.0 - cosine;
    let square = edge * edge;
    return square * square * edge;
}

fn curve_edge(cosine: f32, standard: f32, curve: vec4<f32>, custom: bool) -> f32 {
    if custom && curve.x != 5.0 {
        return pow(1.0 - cosine, curve.x);
    }
    return standard;
}

fn base_fresnel(value: vec3<f32>) -> vec3<f32> {
    if CUSTOM_BASE_FRESNEL {
        return clamp(value * material.fresnel.y + vec3<f32>(material.fresnel.z), vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return value;
}

fn coat_fresnel(cosine: f32, standard: f32, curve: vec4<f32>) -> f32 {
    let edge = curve_edge(cosine, standard, curve, CUSTOM_COAT_FRESNEL);
    let value = 0.04 + 0.96 * edge;
    if CUSTOM_COAT_FRESNEL {
        return clamp(value * curve.y + curve.z, 0.0, 1.0);
    }
    return value;
}

fn ggx_scale(nl: f32, nv: f32, nh: f32, alpha2: f32, smith_v: f32) -> f32 {
    let d_denom = nh * nh * (alpha2 - 1.0) + 1.0;
    let distribution = alpha2 / (3.14159265 * d_denom * d_denom);
    let smith_l = sqrt(nl * nl * (1.0 - alpha2) + alpha2);
    let visibility = 0.5 / max(nl * smith_v + nv * smith_l, 0.00001);
    return distribution * visibility;
}

fn screen_ambient_visibility(vertex: VertexOut) -> f32 {
    if NO_DEPTH_EFFECTS || globals.ao.x <= 0.0 || material.params.w > 0.5 {
        return 1.0;
    }
    let size = vec2<i32>(ambient_occlusion.size);
    let location = vertex.position.xy * vec2<f32>(size) / globals.ao.zw - 0.5;
    let tile = vec2<i32>(floor(location));
    let fraction = fract(location);
    let depth = dot(globals.view_depth, vec4<f32>(vertex.world, 1.0));
    let tolerance = max(globals.ao.y * 0.22, depth * 0.001);
    var weight_sum = 0.0;
    var occlusion = 0.0;
    for (var i = 0u; i < 4u; i += 1u) {
        let delta = vec2<i32>(i32(i & 1u), i32(i >> 1u));
        let pixel = clamp(tile + delta, vec2<i32>(0), size - 1);
        let sample = ambient_occlusion.samples[u32(pixel.y * size.x + pixel.x)];
        if sample.y <= 0.0 {
            continue;
        }
        let kernel = select(1.0 - fraction, fraction, delta != vec2<i32>(0));
        let weight = kernel.x * kernel.y * max(1.0 - abs(sample.y - depth) / tolerance, 0.0);
        occlusion += sample.x * weight;
        weight_sum += weight;
    }
    // Fade out where the low-resolution depth belongs to another surface.
    return 1.0 - globals.ao.x * occlusion / max(weight_sum, 0.35);
}

struct Dielectric {
    f0: vec3<f32>,
    f90: f32,
}

fn sample_dielectric(vertex: VertexOut, uv: UvDerivatives, metallic: f32) -> Dielectric {
    if metallic >= 1.0 {
        return Dielectric(vec3<f32>(0.0), 1.0);
    }
    var weight = material.specular.a * vertex_mask(vertex.color, VERTEX_SPECULAR);
    var tint = material.specular.rgb;
    if weight > 0.0 {
        if material.dielectric.y >= 0.0 {
            let texel = sample_material_map(specular_texture, specular_sampler, vertex, uv,
                material.dielectric.y, material.transforms[4]);
            weight *= scalar_texel(texel, 5u, 3u);
        }
        if material.dielectric.z >= 0.0 {
            let texel = sample_material_map(specular_color_texture, specular_color_sampler, vertex, uv,
                material.dielectric.z, material.transforms[5]);
            tint *= color_texel(texel, 6u).rgb;
        }
    }
    return Dielectric(min(tint * material.dielectric.x, vec3<f32>(1.0)) * weight, weight);
}

fn diffuse_energy(dielectric: Dielectric, edge: f32) -> f32 {
    let fresnel = base_fresnel(mix(dielectric.f0, vec3<f32>(dielectric.f90), edge));
    return 1.0 - max(fresnel.r, max(fresnel.g, fresnel.b));
}

fn transmission_strength(vertex: VertexOut, uv: UvDerivatives, metallic: f32) -> f32 {
    var strength = material.transmission.x * vertex_mask(vertex.color, VERTEX_TRANSMISSION);
    if metallic >= 1.0 || strength <= 0.0 {
        return 0.0;
    }
    if material.transmission.y >= 0.0 {
        let texel = sample_material_map(transmission_texture, transmission_map_sampler,
            vertex, uv, material.transmission.y, material.transforms[6]);
        strength *= scalar_texel(texel, 7u, 0u);
    }
    return strength;
}

fn volume_thickness(vertex: VertexOut, uv: UvDerivatives) -> f32 {
    var thickness = material.volume.x * vertex_mask(vertex.color, VERTEX_THICKNESS);
    if material.volume.y >= 0.0 {
        let texel = sample_material_map(thickness_texture, thickness_sampler, vertex, uv,
            material.volume.y, material.transforms[7]);
        thickness *= scalar_texel(texel, 8u, 1u);
    }
    return thickness;
}

struct VolumeFrame {
    cofactors: mat3x3<f32>,
    determinant: f32,
}

fn volume_frame(vertex: VertexOut) -> VolumeFrame {
    let x = vertex.model_x;
    let y = vertex.model_y;
    let z = vertex.model_z;
    let c0 = cross(y, z);
    return VolumeFrame(mat3x3<f32>(c0, cross(z, x), cross(x, y)), abs(dot(x, c0)));
}

fn volume_ray(frame: VolumeFrame, incident: vec3<f32>, normal: vec3<f32>, inverse_ior: f32,
    thickness: f32) -> vec3<f32> {
    let direction = refract(incident, normal, inverse_ior);
    let to_prepared = direction * frame.cofactors;
    // Compose the inverse glTF node and instance transforms to measure
    // the distance in the mesh space that the thickness map uses.
    let local = vec3<f32>(dot(material.volume_inverse[0].xyz, to_prepared),
        dot(material.volume_inverse[1].xyz, to_prepared),
        dot(material.volume_inverse[2].xyz, to_prepared));
    let length_local = length(local);
    if !(length_local > 0.0 && frame.determinant > 0.0) {
        return vec3<f32>(0.0);
    }
    return direction * (thickness * (frame.determinant / length_local));
}

fn refraction_has_opaque_background(uv: vec2<f32>, clip_depth: f32) -> bool {
    let size = textureDimensions(opaque_radiance);
    if arrayLength(&opaque_depth) != size.x * size.y {
        return false;
    }
    let pixel = min(vec2<u32>(uv * vec2<f32>(size)), size - 1u);
    let nearest = opaque_depth[pixel.y * size.x + pixel.x];
    if globals.camera_right.w > 0.5 {
        let epsilon = max(max(nearest, clip_depth) * 0.000002, 1e-12);
        return nearest <= clip_depth + epsilon;
    }
    return nearest + 0.000002 >= clip_depth;
}

struct RefractedSample {
    uv: vec2<f32>,
    distance: f32,
}

fn refracted_sample(entry: vec2<f32>, position: vec3<f32>, ray: vec3<f32>) -> RefractedSample {
    let distance = length(ray);
    if distance > 0.0 {
        let world = vec4<f32>(position + ray, 1.0);
        let w = dot(globals.clip3, world);
        if w > 0.0 {
            let ndc = vec2<f32>(dot(globals.clip0, world), dot(globals.clip1, world)) / w;
            if all(abs(ndc) <= vec2<f32>(1.0)) {
                let projected = ndc * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
                let exit_depth = dot(globals.clip2, world) / w;
                if refraction_has_opaque_background(projected, exit_depth) {
                    return RefractedSample(projected, distance);
                }
            }
        }
    }
    return RefractedSample(entry, distance);
}

fn volume_attenuation(distance: vec3<f32>) -> vec3<f32> {
    let white = vec3<f32>(1.0);
    if material.absorption.a <= 0.0 || all(material.absorption.rgb == white) {
        return white;
    }
    let exponent = min(distance / material.absorption.a, vec3<f32>(10000.0));
    // Avoid pow(0, 0) for channels whose ray has zero distance.
    let color = pow(material.absorption.rgb, max(exponent, vec3<f32>(1e-30)));
    return select(white, color, distance > vec3<f32>(0.0));
}

struct TransmissionSample {
    radiance: vec3<f32>,
    uncovered: vec3<f32>,
}

fn opaque_light(uv: vec2<f32>, level: f32) -> vec4<f32> {
    return textureSampleLevel(opaque_radiance, transmission_sampler, uv, level);
}

fn sample_transmitted_light(vertex: VertexOut, uv: UvDerivatives, normal: vec3<f32>, view: vec3<f32>,
    roughness: f32) -> TransmissionSample {
    let size = vec2<f32>(textureDimensions(opaque_radiance));
    let entry = vertex.position.xy / size;
    let lod = roughness * floor(log2(max(size.x, size.y)));
    let green_lod = lod * material.transmission.z;
    if VOLUME_MATERIAL {
        let thickness = volume_thickness(vertex, uv);
        if thickness > 0.0 {
            let frame = volume_frame(vertex);
            let green = refracted_sample(entry, vertex.world,
                volume_ray(frame, -view, normal, material.volume.z, thickness));
            let green_light = opaque_light(green.uv, green_lod);
            if DISPERSION_MATERIAL {
                let red = refracted_sample(entry, vertex.world,
                    volume_ray(frame, -view, normal, material.spectral.x, thickness));
                let blue = refracted_sample(entry, vertex.world,
                    volume_ray(frame, -view, normal, material.spectral.y, thickness));
                let r = opaque_light(red.uv, lod * material.spectral.z);
                let b = opaque_light(blue.uv, lod * material.spectral.w);
                let attenuation = volume_attenuation(vec3<f32>(red.distance, green.distance, blue.distance));
                return TransmissionSample(vec3<f32>(r.r, green_light.g, b.b) * attenuation,
                    (vec3<f32>(1.0) - vec3<f32>(r.a, green_light.a, b.a)) * attenuation);
            }
            let attenuation = volume_attenuation(vec3<f32>(green.distance));
            return TransmissionSample(green_light.rgb * attenuation,
                vec3<f32>(1.0 - green_light.a) * attenuation);
        }
    }
    let captured = opaque_light(entry, green_lod);
    return TransmissionSample(captured.rgb, vec3<f32>(1.0 - captured.a));
}

fn baked_irradiance(vertex: VertexOut) -> vec4<f32> {
    let uv = vertex.lightmap_uv;
    if !BAKED_LIGHTMAP || any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0);
    }
    let origin = vertex.lightmap_atlas.xy;
    let extent = vertex.lightmap_atlas.zw;
    let half_texel = min(0.5 / vec2<f32>(textureDimensions(opaque_radiance)), extent * 0.5);
    let at = clamp(origin + uv * extent, origin + half_texel, origin + extent - half_texel);
    let irradiance = textureSampleLevel(opaque_radiance, transmission_sampler, at, 0.0).rgb;
    return vec4<f32>(irradiance * vertex.lightmap_options.x, 1.0 + vertex.lightmap_options.y);
}

struct SurfaceLight {
    color: vec3<f32>,
    transmitted: vec3<f32>,
    background_weight: vec3<f32>,
    uncovered: f32,
}

fn material_view_direction(world: vec3<f32>) -> vec3<f32> {
    if globals.oblique_depth > 1.5 {
        return -custom_forward(world);
    }
    if globals.eye.w < 0.5 {
        return normalize(globals.eye.xyz - world);
    }
    return -globals.view_depth.xyz;
}

fn material_input(vertex: VertexOut, front_facing: bool, view: vec3<f32>) -> XrMaterialInput {
    return XrMaterialInput(vertex.uv, vertex.uv1, vertex.world, view,
        vertex.color, material.shader_parameters.xyz, front_facing, vertex.instance_data,
        vertex.instance_metadata.x >> 3u, vertex.tint);
}

fn apply_surface_program(vertex: VertexOut, front_facing: bool, surface: XrMaterialSurface) -> XrMaterialSurface {
    let input = material_input(vertex, front_facing, material_view_direction(vertex.world));
    var result = scene_material(input, surface);
    let length_squared = dot(result.normal, result.normal);
    if length_squared > 1e-12 && length_squared <= 1e20 {
        result.normal *= inverseSqrt(length_squared);
    } else {
        result.normal = surface.normal;
    }
    if all(abs(result.base_color) <= vec3(1e20)) {
        result.base_color = clamp(result.base_color, vec3(0.0), vec3(65000.0));
    } else {
        result.base_color = surface.base_color;
    }
    if all(abs(result.emissive) <= vec3(1e20)) {
        result.emissive = clamp(result.emissive, vec3(0.0), vec3(65000.0));
    } else {
        result.emissive = surface.emissive;
    }
    result.roughness = select(surface.roughness, clamp(result.roughness, 0.0, 1.0), abs(result.roughness) <= 1e20);
    result.metallic = select(surface.metallic, clamp(result.metallic, 0.0, 1.0), abs(result.metallic) <= 1e20);
    return result;
}

fn valid_program_light(authored: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    return select(fallback, clamp(authored, vec3(0.0), vec3(65000.0)), all(abs(authored) <= vec3(1e20)));
}

var<private> captured_effect_surface: vec4<f32>;
var<private> captured_base_color_metalness: vec4<f32>;
var<private> captured_emissive_occlusion: vec4<f32>;
var<private> captured_material_data: vec4<f32>;

fn effect_surface_sample(normal: vec3<f32>, roughness: f32) -> vec4<f32> {
    let squared = dot(normal, normal);
    var n = vec3(0.0, 0.0, 1.0);
    if squared > 1e-12 && squared <= 1e20 {
        n = normal * inverseSqrt(squared);
    }
    return vec4(n, clamp(roughness, 0.0, 1.0));
}

fn capture_surface(vertex: VertexOut, front_facing: bool, surface: XrMaterialSurface, occlusion: f32) -> vec4<f32> {
    captured_base_color_metalness = vec4(surface.base_color, surface.metallic);
    captured_emissive_occlusion = vec4(surface.emissive, occlusion);
    captured_effect_surface = effect_surface_sample(surface.normal, surface.roughness);
    if CAPTURE_MATERIAL_DATA {
        let input = material_input(vertex, front_facing, material_view_direction(vertex.world));
        let data = scene_material_data(input, surface);
        captured_material_data = select(vec4(0.0), clamp(data, vec4(-65000.0), vec4(65000.0)),
            abs(data) <= vec4(3.402823e38));
    }
    return captured_effect_surface;
}

fn surface_occlusion(vertex: VertexOut, uv: UvDerivatives) -> f32 {
    if material.map_slots.z < 0.0 {
        return 1.0;
    }
    let texel = sample_material_map(occlusion_texture, occlusion_sampler, vertex, uv, material.map_slots.z, material.transforms[2]);
    return mix(1.0, scalar_texel(texel, 4u, 0u),
        material.pbr.z * vertex_mask(vertex.color, VERTEX_OCCLUSION));
}

fn shade_pbr(
    vertex: VertexOut, initial_normal: vec3<f32>, initial_albedo: vec3<f32>, uv: UvDerivatives, coat: CoatSurface,
    specular_filter: f32, brush: TangentFrame, front_facing: bool,
) -> SurfaceLight {
    let pi = 3.14159265;
    let glossy = material.pbr.w > 1.5;
    let roughness_mask = vertex_mask(vertex.color, VERTEX_ROUGHNESS);
    var metallic = material.pbr.x * vertex_mask(vertex.color, VERTEX_METALNESS);
    var roughness = material.pbr.y * roughness_mask;
    if material.map_slots.x >= 0.0 {
        let texel = sample_material_map(mr_texture, mr_sampler, vertex, uv, material.map_slots.x, material.transforms[0]);
        if glossy {
            roughness = (1.0 - (1.0 - material.pbr.y) * scalar_texel(texel, 11u, 3u))
                * roughness_mask;
        } else {
            // glTF stores roughness in G and metallic in B, both linear.
            roughness *= scalar_texel(texel, 3u, 1u);
            metallic *= scalar_texel(texel, 2u, 2u);
        }
    }
    var emissive = material.emissive.rgb;
    if material.map_slots.y >= 0.0 {
        let texel = sample_material_map(emissive_texture, emissive_sampler, vertex, uv, material.map_slots.y, material.transforms[1]);
        emissive *= color_texel(texel, 1u).rgb;
    }
    var n = initial_normal;
    var albedo = initial_albedo;
    if CUSTOM_MATERIAL {
        let surface = apply_surface_program(vertex, front_facing,
            XrMaterialSurface(albedo, emissive, n, roughness, metallic));
        albedo = surface.base_color;
        emissive = surface.emissive;
        n = surface.normal;
        roughness = surface.roughness;
        metallic = surface.metallic;
    }
    var occlusion = 1.0;
    if !CAPTURE_EFFECT_SURFACE || CAPTURE_MATERIAL_OCCLUSION {
        occlusion = surface_occlusion(vertex, uv);
    }
    if CAPTURE_EFFECT_SURFACE || CAPTURE_LIT_SURFACE {
        _ = capture_surface(vertex, front_facing, XrMaterialSurface(albedo, emissive, n, roughness, metallic), occlusion);
    }
    if CAPTURE_EFFECT_SURFACE {
        return SurfaceLight(vec3(0.0), vec3(0.0), vec3(0.0), 0.0);
    }
    let transmission_roughness = roughness;
    roughness = filtered_roughness(roughness, specular_filter);
    let dielectric = sample_dielectric(vertex, uv, metallic);
    let f0 = mix(dielectric.f0, albedo, metallic);
    let f90 = mix(dielectric.f90, 1.0, metallic);
    let v = material_view_direction(vertex.world);
    let nv = clamp(dot(n, v), 0.0, 1.0);
    let transmission = transmission_strength(vertex, uv, metallic);
    let base_albedo = albedo * (1.0 - metallic);
    let diffuse_albedo = base_albedo * (1.0 - transmission);
    let view_edge = curve_edge(nv, fresnel_edge(nv), material.fresnel, CUSTOM_BASE_FRESNEL);
    let energy = diffuse_energy(dielectric, view_edge);
    let ambient_diffuse = diffuse_albedo * energy;
    let shading = XrLightingInput(material_input(vertex, front_facing, v),
        XrMaterialSurface(albedo, emissive, n, roughness, metallic), f0);
    var transmitted = vec3<f32>(0.0);
    var uncovered = 0.0;
    let background_weight = (transmission * energy) * base_albedo;
    if transmission > 0.0 {
        let sample = sample_transmitted_light(vertex, uv, n, v, transmission_roughness);
        transmitted = sample.radiance * (transmission * energy) * base_albedo;
        if globals.background_alpha < 1.0 && globals.volume_background.w < 0.5 {
            // A 2D underlay can contribute behind the uncovered part of the
            // captured scene. One alpha represents the transmitted luminance.
            uncovered = dot(sample.uncovered * background_weight, vec3<f32>(0.2126, 0.7152, 0.0722));
        }
    }
    let constant = (ambient_diffuse * 0.22 + albedo * metallic * 0.065) * lighting.ambient.rgb;
    let environment_irradiance = environment_diffuse(n);
    let diffuse_env = environment_irradiance * ambient_diffuse;
    var indirect_diffuse = ambient_diffuse * (lighting.ambient.rgb * 0.22) + diffuse_env;
    var lobe = AnisotropicLobe(vec3<f32>(0.0), vec3<f32>(0.0), 0.0);
    var reflection_normal = n;
    if ANISOTROPIC_MATERIAL {
        lobe = brush_direction(vertex, n, uv, brush);
        reflection_normal = anisotropic_probe_normal(n, v, lobe, roughness);
    }
    let reflection = environment_specular(vertex.world, reflect(-v, reflection_normal), f0, f90, nv, roughness,
        material.fresnel, CUSTOM_BASE_FRESNEL);
    var ambient = (constant + diffuse_env + reflection) * occlusion;
    // The clear layer is a dielectric with IOR 1.5 (F0 = 0.04).
    let coat_nv = clamp(dot(coat.normal, v), 0.0, 1.0);
    let ao = screen_ambient_visibility(vertex);
    if XRACK_AMBIENT_PROGRAM {
        let input = XrAmbientInput(shading, lighting.ambient.rgb * 0.22 + environment_irradiance, occlusion * ao);
        indirect_diffuse = valid_program_light(scene_ambient(input, indirect_diffuse), indirect_diffuse);
        let ambient_specular = (albedo * metallic * 0.065) * lighting.ambient.rgb + reflection;
        ambient = (indirect_diffuse + ambient_specular) * occlusion;
    }
    var base_attenuation = 1.0;
    var coat_reflection = vec3<f32>(0.0);
    if coat.amount > 0.0 {
        let weight = coat.amount * coat_fresnel(coat_nv, fresnel_edge(coat_nv), coat.fresnel);
        base_attenuation = 1.0 - weight;
        coat_reflection = environment_specular(vertex.world, reflect(-v, coat.normal),
            vec3<f32>(0.04), 1.0, coat_nv, coat.roughness, coat.fresnel, CUSTOM_COAT_FRESNEL);
        ambient = ambient * base_attenuation + coat_reflection * (coat.amount * occlusion);
        emissive *= base_attenuation;
        transmitted *= base_attenuation;
    }
    let baked = baked_irradiance(vertex);
    let existing_diffuse = indirect_diffuse * (occlusion * base_attenuation * ao);
    let replace_diffuse = select(vec3<f32>(0.0),
        baked.rgb * ambient_diffuse * base_attenuation - existing_diffuse, baked.a > 0.0);
    if SCREEN_INDIRECT_LIGHT {
        let baked_surface = baked.a > 0.0;
        let attenuation = ao * occlusion * base_attenuation;
        record_diffuse(n, select(ambient_diffuse * attenuation, vec3<f32>(0.0), baked_surface),
            select(existing_diffuse, vec3<f32>(0.0), baked_surface));
    }
    if SCREEN_REFLECTIONS && nv > 0.0 {
        let base_weight = reflection_response(f0, f90, nv, roughness, material.fresnel, CUSTOM_BASE_FRESNEL) * base_attenuation;
        let coat_weight = reflection_response(vec3<f32>(0.04), 1.0, coat_nv, coat.roughness, coat.fresnel, CUSTOM_COAT_FRESNEL) * coat.amount;
        let visibility = ao * occlusion;
        // A single ray follows the lobe most likely to resolve visible detail.
        // The other lobe remains in the probe-lit color.
        if coat.amount > 0.0 && coat_nv > 0.0
            && dot(coat_weight, vec3<f32>(1.0)) * (1.0 - coat.roughness)
                > dot(base_weight, vec3<f32>(1.0)) * (1.0 - roughness) {
            record_reflection(coat.normal, coat.roughness, coat_weight * visibility,
                coat_reflection * (coat.amount * visibility));
        } else {
            record_reflection(reflection_normal, roughness, base_weight * visibility,
                reflection * (base_attenuation * visibility));
        }
    }
    if nv <= 0.0 && (coat.amount == 0.0 || coat_nv <= 0.0) {
        return SurfaceLight(ambient * ao + replace_diffuse + emissive + transmitted,
            transmitted, background_weight * base_attenuation, uncovered * base_attenuation);
    }
    let r = max(roughness, 0.08);
    let r2 = r * r;
    let alpha2 = r2 * r2;
    let smith_v = sqrt(nv * nv * (1.0 - alpha2) + alpha2);
    let coat_r = max(coat.roughness, 0.08);
    let coat_r2 = coat_r * coat_r;
    let coat_alpha2 = coat_r2 * coat_r2;
    let coat_smith_v = sqrt(coat_nv * coat_nv * (1.0 - coat_alpha2) + coat_alpha2);
    let full_diffuse_bake = baked.a == 2.0;
    let mixed_bake = baked.a == 3.0;
    let diffuse_factor = select(diffuse_albedo / pi, vec3<f32>(0.0), full_diffuse_bake);
    var direct = vec3<f32>(0.0);
    for (var index = 0u; index < u32(lighting.settings.x); index += 1u) {
        let light = lighting.lights[index];
        if BAKED_LIGHTMAP && mixed_bake && light.cone.z > 0.5 {
            continue;
        }
        let incident = incident_light(light, vertex.world);
        let l = incident.axis;
        let nl = clamp(dot(n, l), 0.0, 1.0);
        let coat_nl = clamp(dot(coat.normal, l), 0.0, 1.0);
        let base_lit = nl > 0.0 && nv > 0.0;
        let coat_lit = coat.amount > 0.0 && coat_nl > 0.0 && coat_nv > 0.0;
        let programmable = XRACK_DIRECT_LIGHT_PROGRAM && nv > 0.0 && incident.attenuation > 0.0;
        if !base_lit && !coat_lit && !programmable {
            continue;
        }
        // The custom lobe may accept light from behind the normal. Use a
        // finite half vector when the light and eye directions cancel.
        var h = normalize(v + l);
        if XRACK_DIRECT_LIGHT_PROGRAM && dot(v + l, v + l) <= 1e-12 {
            h = n;
        }
        let vh = clamp(dot(v, h), 0.0, 1.0);
        let edge = fresnel_edge(vh);
        var base_edge = 0.0;
        var fresnel = vec3<f32>(0.0);
        var response = vec3<f32>(0.0);
        var standard = XrLightResponse(vec3(0.0), vec3(0.0));
        if base_lit || programmable {
            base_edge = curve_edge(vh, edge, material.fresnel, CUSTOM_BASE_FRESNEL);
            fresnel = base_fresnel(mix(f0, vec3<f32>(f90), base_edge));
        }
        if base_lit {
            let nh = clamp(dot(n, h), 0.0, 1.0);
            let diffuse = diffuse_energy(dielectric, base_edge) * diffuse_factor;
            var microfacet: f32;
            if ANISOTROPIC_MATERIAL {
                microfacet = anisotropic_scale(lobe, l, v, h, nl, nv, nh, r2, smith_v);
            } else {
                microfacet = ggx_scale(nl, nv, nh, alpha2, smith_v);
            }
            let specular = fresnel * microfacet;
            response = (diffuse + specular) * nl;
            if programmable {
                standard = XrLightResponse(diffuse * nl, specular * nl);
            }
        }
        let shadow = shadow_visibility(light, vertex);
        if programmable {
            let input = XrLightInput(shading, u32(light.position_kind.w), index, l,
                light.radiance.xyz, incident.attenuation, shadow, fresnel);
            let authored = scene_light(input, standard);
            let diffuse_response = valid_program_light(authored.diffuse, standard.diffuse);
            let specular_response = valid_program_light(authored.specular, standard.specular);
            response = select(diffuse_response, vec3(0.0), full_diffuse_bake) + specular_response;
        }
        if coat_lit {
            let nh = clamp(dot(coat.normal, h), 0.0, 1.0);
            let weight = coat.amount * coat_fresnel(vh, edge, coat.fresnel);
            let coat_specular = ggx_scale(coat_nl, coat_nv, nh, coat_alpha2, coat_smith_v);
            response = response * (1.0 - weight) + vec3<f32>(coat_specular * weight * coat_nl);
        }
        direct += response * (shadow * incident.radiance);
    }
    return SurfaceLight(ambient * ao + replace_diffuse + emissive + direct + transmitted,
            transmitted, background_weight * base_attenuation, uncovered * base_attenuation);
}

fn shade_builtin(vertex: VertexOut, normal: vec3<f32>) -> vec3<f32> {
    let ambient = screen_ambient_visibility(vertex);
    let tint = srgb_to_linear(vertex.tint.rgb);
    if lighting.settings.y < 0.5 {
        let cosine = max(dot(normal, lighting.lights[0].axis_inverse_range.xyz), 0.0);
        let shadow = shadow_visibility(lighting.lights[0], vertex);
        let direct = vec3<f32>(0.64 * cosine * shadow);
        let brightness = lighting.ambient.rgb * (0.36 * ambient) + direct;
        let environment = tint * environment_diffuse(normal) * ambient;
        let lit = srgb_to_linear(vertex.tint.rgb * brightness) + environment;
        if SCREEN_INDIRECT_LIGHT {
            let diffuse_ambient = max(lit - srgb_to_linear(vertex.tint.rgb * direct), vec3<f32>(0.0));
            record_diffuse(normal, tint * ambient, diffuse_ambient);
        }
        return lit;
    }
    let indirect = (lighting.ambient.rgb * 0.22 + environment_diffuse(normal)) * ambient;
    var irradiance = indirect;
    if SCREEN_INDIRECT_LIGHT {
        record_diffuse(normal, tint * ambient, tint * indirect);
    }
    for (var index = 0u; index < u32(lighting.settings.x); index += 1u) {
        let incident = incident_light(lighting.lights[index], vertex.world);
        let cosine = max(dot(normal, incident.axis), 0.0);
        if cosine > 0.0 {
            let shadow = shadow_visibility(lighting.lights[index], vertex);
            irradiance += incident.radiance * (cosine * shadow / 3.14159265);
        }
    }
    return tint * irradiance;
}

@fragment
fn fragment_main(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return plain_surface(vertex, front_facing);
}

@fragment
fn fragment_coated(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    return coated_surface(vertex, front_facing);
}

@fragment
fn fragment_weighted(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> WeightedColor {
    return weighted_color(plain_surface(vertex, front_facing), vertex.world);
}

@fragment
fn fragment_weighted_coated(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> WeightedColor {
    return weighted_color(coated_surface(vertex, front_facing), vertex.world);
}

fn with_surfaces(color: vec4<f32>) -> ReflectedColor {
    var normal = screen_reflection.normal_roughness;
    var response = screen_reflection.response;
    if PACK_REFLECTION_ATTRIBUTES && dot(normal.xyz, normal.xyz) >= 0.5 {
        normal = vec4<f32>(normal.xyz * 0.5 + 0.5, normal.w);
        // Preserve precision around the Fresnel response of dielectrics.
        response = sqrt(response);
    }
    return ReflectedColor(color, normal, vec4<f32>(response, 0.0),
        vec4<f32>(screen_reflection.probe, 0.0));
}

@fragment
fn fragment_screen(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> ReflectedColor {
    return with_surfaces(plain_surface(vertex, front_facing));
}

@fragment
fn fragment_screen_coat(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> ReflectedColor {
    return with_surfaces(coated_surface(vertex, front_facing));
}

fn pack_response(response: vec3<f32>) -> vec3<u32> {
    return vec3<u32>(round(255.0 * sqrt(clamp(response, vec3<f32>(0.0), vec3<f32>(1.0)))));
}

fn with_indirect(color: vec4<f32>) -> IndirectColor {
    let specular = screen_reflection.normal_roughness;
    var spec_code = 0u;
    var diff_code = 0u;
    var flags = 0u;
    if dot(specular.xyz, specular.xyz) > 0.5 {
        spec_code = pack_screen_normal(specular.xyz);
        flags = 16384u | u32(round(clamp(specular.w, 0.0, 1.0) * 16383.0));
    }
    if dot(screen_diffuse.normal, screen_diffuse.normal) > 0.5 {
        diff_code = pack_screen_normal(screen_diffuse.normal);
        flags |= 32768u;
    }
    let ambient = pack_ambient(screen_diffuse.ambient);
    let factors = pack_response(screen_reflection.response) | (pack_response(screen_diffuse.response) << vec3<u32>(8u));
    return IndirectColor(
        color,
        vec4<u32>(spec_code, diff_code, flags, ambient & 65535u),
        vec4<u32>(factors, ambient >> 16u),
        vec4<f32>(screen_reflection.probe, max(color.r, max(color.g, color.b))),
    );
}

@fragment
fn fragment_indirect(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> IndirectColor {
    return with_indirect(plain_surface(vertex, front_facing));
}

@fragment
fn fragment_indirect_coat(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> IndirectColor {
    return with_indirect(coated_surface(vertex, front_facing));
}

struct WeightedColor {
    @location(0) accumulation: vec4<f32>,
    @location(1) opacity: f32,
}

fn weighted_color(premultiplied: vec4<f32>, world: vec3<f32>) -> WeightedColor {
    // Positive linear camera depth makes weighting independent of projection
    // type. Scale by the camera's near/far geometric mean.
    let depth = clamp(dot(globals.view_depth, vec4<f32>(world, 1.0)) * globals.oit_depth_scale, 0.0, 128.0);
    let importance = (0.25 + 0.75 * premultiplied.a) / (1.0 + depth * depth);
    // Leave headroom for many overlapping HDR fragments in the FP16 target.
    let weight = max(importance, 0.0078125) * 0.03125;
    return WeightedColor(premultiplied * weight, premultiplied.a);
}

fn plain_surface(vertex: VertexOut, front_facing: bool) -> vec4<f32> {
    // XRack wireframe distance
    var uv: UvDerivatives;
    if material.pbr.w > 0.5 && (any(material.map_slots >= vec4<f32>(0.0)) ||
        any(material.dielectric.yz >= vec2<f32>(0.0)) || material.transmission.y >= 0.0 ||
        (VOLUME_MATERIAL && material.volume.y >= 0.0)) {
        uv = uv_derivatives(vertex);
    }
    let color = fragment_surface(vertex, front_facing, uv, CoatSurface(vec3<f32>(0.0), 0.0, 0.0, vec4<f32>(5.0, 1.0, 0.0, 0.0)));
    // XRack wireframe discard
    return color;
}

fn coated_surface(vertex: VertexOut, front_facing: bool) -> vec4<f32> {
    // XRack wireframe distance
    let uv = uv_derivatives(vertex);
    let coat = coat_surface(vertex, uv, front_facing);
    let color = fragment_surface(vertex, front_facing, uv, coat);
    // XRack wireframe discard
    return color;
}

struct CaptureGeometry {
    @location(0) normal_roughness: vec4<f32>,
    @location(1) base_color_metalness: vec4<f32>,
    @location(2) emissive_occlusion: vec4<f32>,
    @location(3) depth: f32,
}

@fragment
fn fragment_capture_geometry(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> CaptureGeometry {
    let normal_roughness = plain_surface(vertex, front_facing);
    return CaptureGeometry(normal_roughness, captured_base_color_metalness, captured_emissive_occlusion, vertex.position.z);
}

struct LitCapture {
    @location(0) normal_roughness: vec4<f32>,
    @location(1) base_color_metalness: vec4<f32>,
    @location(2) emissive_occlusion: vec4<f32>,
    @location(3) depth: f32,
    @location(4) color: vec4<f32>,
}

fn lit_capture(vertex: VertexOut, color: vec4<f32>) -> LitCapture {
    return LitCapture(captured_effect_surface, captured_base_color_metalness,
        captured_emissive_occlusion, vertex.position.z, color);
}

@fragment
fn fragment_capture_lit(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> LitCapture {
    return lit_capture(vertex, plain_surface(vertex, front_facing));
}

@fragment
fn fragment_capture_lit_coat(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> LitCapture {
    return lit_capture(vertex, coated_surface(vertex, front_facing));
}

struct LitDataCapture {
    @location(0) normal_roughness: vec4<f32>,
    @location(1) base_color_metalness: vec4<f32>,
    @location(2) emissive_occlusion: vec4<f32>,
    @location(3) depth: f32,
    @location(4) color: vec4<f32>,
    @location(5) data: vec4<f32>,
}

fn lit_data_capture(vertex: VertexOut, color: vec4<f32>) -> LitDataCapture {
    let lit = lit_capture(vertex, color);
    return LitDataCapture(lit.normal_roughness, lit.base_color_metalness, lit.emissive_occlusion,
        lit.depth, lit.color, captured_material_data);
}

@fragment
fn fragment_capture_lit_data(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> LitDataCapture {
    return lit_data_capture(vertex, plain_surface(vertex, front_facing));
}

@fragment
fn fragment_capture_lit_data_coat(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> LitDataCapture {
    return lit_data_capture(vertex, coated_surface(vertex, front_facing));
}

@fragment
fn fragment_capture_data(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    _ = plain_surface(vertex, front_facing);
    return captured_material_data;
}

@fragment
fn fragment_capture_depth(vertex: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(3) f32 {
    // Run the same fragment coverage test as a normal MRT capture. The
    // unused surface shading result can be stripped from this entry point.
    _ = plain_surface(vertex, front_facing);
    return vertex.position.z;
}

fn uv_derivatives(vertex: VertexOut) -> UvDerivatives {
    return UvDerivatives(dpdx(vertex.uv), dpdy(vertex.uv), dpdx(vertex.uv1), dpdy(vertex.uv1));
}

fn fog_curve(amount: f32, curve: f32) -> f32 {
    if curve == 1.0 {
        return amount;
    }
    return pow(amount, curve);
}

fn fragment_surface(vertex: VertexOut, front_facing: bool, uv: UvDerivatives, coat: CoatSurface) -> vec4<f32> {
    let normal = normalize(vertex.normal);
    let filter_specular = material.pbr.w > 0.5 && globals.specular_aa > 0.5;
    var tangent = TangentFrame(vertex.tangent, bitcast<f32>(vertex.instance_metadata.y));
    var brush = tangent;
    if ANISOTROPIC_MATERIAL {
        // Take derivatives before alpha-cut fragments diverge.
        let slot = max(0.0, material.anisotropy.w);
        let derived = alternate_tangent_frame(vertex, normal, uv, slot, material.anisotropy_uv);
        if abs(derived.sign) > 0.5 && (material.anisotropy.w >= 0.0 || abs(brush.sign) < 0.5) {
            brush = derived;
        }
    }
    let normal_strength = material.emissive.w * vertex_mask(vertex.color, VERTEX_NORMAL);
    if !TANGENT_VERTEX && material.map_slots.w >= 0.0 && material.emissive.w != 0.0 {
        tangent = alternate_tangent_frame(vertex, normal, uv, material.map_slots.w, material.transforms[3]);
    }
    var mapped = normal;
    var specular_filter = 0.0;
    if filter_specular {
        // Derivatives require the whole fragment quad, including alpha-cut fragments.
        mapped = mapped_normal(vertex, normal, uv, tangent,
            normal_texture, normal_sampler, material.map_slots.w, normal_strength, material.transforms[3]);
        specular_filter = specular_kernel(mapped);
    }
    var base: vec3<f32>;
    var alpha = 1.0;
    var transmitted = vec3<f32>(0.0);
    var background_weight = vec3<f32>(0.0);
    var uncovered = 0.0;
    if material.params.y > 0.5 {
        var texel = vec4<f32>(1.0);
        if material.params.z > 0.5 {
            var base_sample = textureSample(base_texture, base_sampler, texture_uv(material.base_uv, vertex.uv));
            if material.params.z > 1.5 {
                // Live layers use premultiplied alpha. A mip view decodes
                // premultiplied linear RGB in hardware.
                var straight = clamp(base_sample.rgb / max(base_sample.a, 1e-6), vec3<f32>(0.0), vec3<f32>(1.0));
                if material.params.z < 2.5 {
                    straight = srgb_to_linear(straight);
                }
                base_sample = vec4<f32>(straight, base_sample.a);
            }
            texel = color_texel(base_sample, 0u);
        }
        let vertex_color = select(vec4<f32>(1.0), vertex.color, USE_VERTEX_COLORS);
        let surface = material.base * vertex_color * texel;
        let coverage = material_coverage(material.params.w, surface.a, vertex.tint.a);
        let cutout = select(coverage.y, surface.a, MODEL_PARTICLES);
        if material.params.x >= 0.0 && cutout < material.params.x {
            discard;
        }
        if material.params.w > 0.5 {
            alpha = coverage.x;
            if alpha <= 0.0 {
                discard;
            }
        } else if MODEL_PARTICLES {
            alpha = vertex.tint.a;
        }
        let albedo = srgb_to_linear(vertex.tint.rgb) * surface.rgb;
        if material.pbr.w > 0.5 {
            if !filter_specular {
                mapped = mapped_normal(vertex, normal, uv, tangent,
                    normal_texture, normal_sampler, material.map_slots.w, normal_strength, material.transforms[3]);
            }
            let lighting = shade_pbr(vertex, select(-mapped, mapped, front_facing), albedo, uv, coat, specular_filter, brush, front_facing);
            if CAPTURE_EFFECT_SURFACE {
                return captured_effect_surface;
            }
            base = lighting.color;
            transmitted = lighting.transmitted;
            background_weight = lighting.background_weight;
            uncovered = lighting.uncovered;
        } else {
            let oriented = select(-normal, normal, front_facing);
            var surface = XrMaterialSurface(albedo, vec3(0.0), oriented, 1.0, 0.0);
            if CUSTOM_MATERIAL {
                surface = apply_surface_program(vertex, front_facing, surface);
            }
            if CAPTURE_EFFECT_SURFACE || CAPTURE_LIT_SURFACE {
                let captured = capture_surface(vertex, front_facing, surface, 1.0);
                if CAPTURE_EFFECT_SURFACE {
                    return captured;
                }
            }
            base = surface.base_color + surface.emissive;
            if SCREEN_INDIRECT_LIGHT {
                record_diffuse(select(-normal, normal, front_facing), vec3<f32>(0.0), vec3<f32>(0.0));
            }
        }
    } else {
        if CAPTURE_EFFECT_SURFACE || CAPTURE_LIT_SURFACE {
            let oriented = select(-normal, normal, front_facing);
            let captured = capture_surface(vertex, front_facing,
                XrMaterialSurface(srgb_to_linear(vertex.tint.rgb), vec3(0.0), oriented, 1.0, 0.0), 1.0);
            if CAPTURE_EFFECT_SURFACE {
                return captured;
            }
        }
        base = shade_builtin(vertex, normal);
        if MODEL_PARTICLES || material.params.w > 0.5 {
            alpha = vertex.tint.a;
        }
    }
    if (vertex.instance_metadata.x & 1u) != 0u {
        if SCREEN_REFLECTIONS {
            screen_reflection.normal_roughness = vec4<f32>(0.0);
        }
        if SCREEN_INDIRECT_LIGHT {
            screen_diffuse.response = vec3<f32>(0.0);
            screen_diffuse.ambient = vec3<f32>(0.0);
        }
        let highlighted = min(linear_to_srgb(base) * 0.75 + vec3<f32>(0.24), vec3<f32>(1.0));
        base = srgb_to_linear(highlighted);
    }
    if globals.fog.a > 0.0 {
        var depth_mix = 0.0;
        var depth_amount = 0.0;
        if globals.fog_depth.y > 0.0 {
            let depth = dot(globals.view_depth, vec4<f32>(vertex.world, 1.0));
            depth_mix = clamp((depth - globals.fog_depth.x) * globals.fog_depth.y, 0.0, 1.0);
            depth_amount = fog_curve(depth_mix, globals.fog_depth.z);
        }
        var height_amount = 0.0;
        if globals.fog_height.y != 0.0 {
            let height = (vertex.world.y - globals.fog_height.x) * globals.fog_height.y;
            height_amount = fog_curve(smoothstep(0.0, 1.0, height), globals.fog_height.z);
        }
        let fog = max(depth_amount, height_amount) * globals.fog.a;
        var fog_color = globals.fog.rgb;
        var surface_weight = vec3<f32>(1.0 - fog);
        if globals.fog_depth.w > 0.0 {
            let transmission = 1.0 - fog_curve(depth_mix, globals.fog_depth.w);
            let bright = base > fog_color;
            fog_color += max(base - fog_color, vec3<f32>(0.0)) * transmission;
            // Preserve the local slope of the color transfer for screen-space lighting.
            surface_weight += select(vec3<f32>(0.0), vec3<f32>(fog * transmission), bright);
        }
        base = mix(base, fog_color, fog);
        transmitted *= surface_weight;
        background_weight *= surface_weight;
        uncovered *= 1.0 - fog;
        if SCREEN_REFLECTIONS {
            screen_reflection.response *= surface_weight;
            screen_reflection.probe *= surface_weight;
        }
        if SCREEN_INDIRECT_LIGHT {
            screen_diffuse.response *= surface_weight;
            screen_diffuse.ambient *= surface_weight;
        }
    }
    if VOLUMETRIC_FOG {
        let fog = fog_to_surface(vertex.world);
        // The screen-space transmission sample has already traversed foreground fog.
        base = base * fog.transmission
            + transmitted * (1.0 - fog.transmission)
            + fog.radiance * (vec3<f32>(1.0) - background_weight);
        if SCREEN_REFLECTIONS {
            screen_reflection.response *= fog.transmission;
            screen_reflection.probe *= fog.transmission;
        }
        if SCREEN_INDIRECT_LIGHT {
            screen_diffuse.response *= fog.transmission;
            screen_diffuse.ambient *= fog.transmission;
        }
    }
    if NO_DEPTH_EFFECTS {
        if SCREEN_REFLECTIONS {
            screen_reflection = ReflectionData(vec4<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
        }
        if SCREEN_INDIRECT_LIGHT {
            screen_diffuse = DiffuseScreenData(vec3<f32>(0.0), vec3<f32>(0.0), vec3<f32>(0.0));
        }
    }
    if LIMIT_BLEND_TINT {
        base = clamp(base, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(min(base, vec3<f32>(65000.0)) * alpha, alpha * (1.0 - clamp(uncovered, 0.0, 1.0)));
}
