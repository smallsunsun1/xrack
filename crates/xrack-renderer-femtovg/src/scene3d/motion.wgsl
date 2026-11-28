override TEST_COVERAGE: bool = false;
override LOD_FADE: bool = false;
override TRACK_SURFACE: bool = true;

struct Projection {
    clip: mat4x4<f32>,
    depth: vec4<f32>,
}

struct Cameras {
    current: Projection,
    previous: Projection,
    extent: vec4<f32>,
}

struct PreviousInstance {
    x: vec4<f32>,
    y: vec4<f32>,
    z: vec4<f32>,
    previous_id: u32,
    index_and_enabled: u32,
    scale: f32,
    tint: u32,
    instance_data: vec4<f32>,
}

struct Material {
    base: vec4<f32>,
    params: vec4<f32>,
    base_uv: TextureTransform,
    channels: vec4<u32>,
    shader_parameters: vec4<f32>,
    opacity_uv: TextureTransform,
    opacity_sampling: vec4<u32>,
    primitive_sizes: vec4<f32>,
}

@group(0) @binding(0) var<uniform> cameras: Cameras;
@group(0) @binding(1) var<storage, read> previous_instances: array<PreviousInstance>;
@group(0) @binding(2) var<uniform> previous_vertex_parameters: vec4<f32>;

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var base_sampler: sampler;

@group(2) @binding(0) var<uniform> previous_skin: JointPalette;
@group(2) @binding(1) var<uniform> previous_morph: MorphSettings;

struct Vertex {
    @builtin(vertex_index) index: u32,
    @location(0) point: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
    @location(4) x: vec4<f32>,
    @location(5) y: vec4<f32>,
    @location(6) z: vec4<f32>,
    @location(7) tint: vec4<f32>,
    @location(8) metadata: vec2<u32>,
    @location(9) uv1: vec2<f32>,
    @location(15) instance_data: vec4<f32>,
}

struct RasterVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) previous_clip: vec4<f32>,
    @location(1) depth: vec2<f32>,
    @location(2) uv: vec4<f32>,
    @location(3) alpha: vec2<f32>,
    @location(4) @interpolate(flat) ids: vec2<u32>,
    @location(5) @interpolate(flat) scale: f32,
    @location(6) @interpolate(flat) metadata: u32,
    // XRack wireframe varying
}

fn motion_world(vertex: Vertex, local: vec3<f32>) -> vec4<f32> {
    let position = vec4(xr_displaced_vertex(local, material.shader_parameters, vertex.instance_data,
        xr_instance_number(vertex.metadata.x), vertex.tint, mat3x4(vertex.x, vertex.y, vertex.z)), 1.0);
    return vec4(dot(vertex.x, position), dot(vertex.y, position), dot(vertex.z, position), 1.0);
}

fn previous_motion_world(old: PreviousInstance, local: vec3<f32>) -> vec4<f32> {
    let tint = vec4<f32>((vec4<u32>(old.tint) >> vec4<u32>(24u, 16u, 8u, 0u)) & vec4<u32>(255u)) / 255.0;
    let position = vec4(xr_displaced_vertex(local, previous_vertex_parameters, old.instance_data,
        old.index_and_enabled >> 1u, tint, mat3x4(old.x, old.y, old.z)), 1.0);
    return vec4(dot(old.x, position), dot(old.y, position), dot(old.z, position), 1.0);
}

fn paired_vertex(vertex: Vertex, index: u32, current: vec3<f32>, previous: vec3<f32>) -> RasterVertex {
    let old = previous_instances[index];
    let world = motion_world(vertex, current);
    let previous_world = previous_motion_world(old, previous);
    var current_clip = xr_point_corner(world * cameras.current.clip, vertex.index, material.primitive_sizes.x, cameras.extent.xy);
    var previous_clip = xr_point_corner(previous_world * cameras.previous.clip, vertex.index, material.primitive_sizes.x, cameras.extent.xy);
    // XRack widen motion
    return RasterVertex(
        current_clip,
        previous_clip,
        vec2<f32>(dot(cameras.current.depth, world), dot(cameras.previous.depth, previous_world)),
        vec4(vertex.uv, vertex.uv1),
        vec2<f32>(vertex.color.a, vertex.tint.a),
        vec2<u32>(select(0u, index + 1u, (old.index_and_enabled & 1u) != 0u), old.previous_id),
        old.scale,
        vertex.metadata.x,
        // XRack wireframe corner
    );
}

fn previous_skin_point(weights: SkinWeights, point: vec3<f32>) -> vec3<f32> {
    var rows: SkinRows;
    for (var i = 0u; i < 4u; i += 1u) {
        let a = previous_skin.rows[weights.joints0[i]];
        let b = previous_skin.rows[weights.joints1[i]];
        rows.x += weights.weights0[i] * a.x + weights.weights1[i] * b.x;
        rows.y += weights.weights0[i] * a.y + weights.weights1[i] * b.y;
        rows.z += weights.weights0[i] * a.z + weights.weights1[i] * b.z;
    }
    return skin_point(rows, point);
}

fn previous_morph_point(vertex: u32, point: vec3<f32>) -> vec3<f32> {
    let index = morph_vertex_row(vertex);
    var result = point;
    let end = morph_entries[index + 1u];
    for (var entry = morph_entries[index]; entry < end; entry += 1u) {
        let offset = previous_morph.geometry.x + entry * previous_morph.geometry.y;
        let target_index = morph_entries[offset];
        let weight = previous_morph.weights[target_index / 4u][target_index % 4u];
        result += weight * morph_vector(offset + 1u);
    }
    return result;
}

@vertex
fn rigid(vertex: Vertex, @builtin(instance_index) instance: u32) -> RasterVertex {
    return paired_vertex(vertex, instance, vertex.point, vertex.point);
}

@vertex
fn skinned(vertex: Vertex, weights: SkinWeights, @builtin(instance_index) instance: u32) -> RasterVertex {
    return paired_vertex(vertex, instance,
        skin_point(skin_transform(weights), vertex.point), previous_skin_point(weights, vertex.point));
}

fn morphed_coverage(vertex: Vertex, delta: MorphDisplacement) -> Vertex {
    var result = vertex;
    result.uv += delta.uvs.xy;
    result.uv1 += delta.uvs.zw;
    result.color.a = clamp(vertex.color.a + delta.color.a, 0.0, 1.0);
    return result;
}

@vertex
fn morphed(vertex: Vertex, @builtin(instance_index) instance: u32) -> RasterVertex {
    let delta = morph_displacement(vertex.index, select(0u, MORPH_COVERAGE, TEST_COVERAGE));
    return paired_vertex(morphed_coverage(vertex, delta), instance,
        vertex.point + delta.position, previous_morph_point(vertex.index, vertex.point));
}

@vertex
fn skinned_morphed(vertex: Vertex, weights: SkinWeights,
                  @builtin(instance_index) instance: u32) -> RasterVertex {
    let delta = morph_displacement(vertex.index, select(0u, MORPH_COVERAGE, TEST_COVERAGE));
    return paired_vertex(morphed_coverage(vertex, delta), instance,
        skin_point(skin_transform(weights), vertex.point + delta.position),
        previous_skin_point(weights, previous_morph_point(vertex.index, vertex.point)));
}

struct MotionOutput {
    @location(0) vector: vec4<f32>,
    @location(1) id: u32,
}

@fragment
fn fragment(vertex: RasterVertex) -> MotionOutput {
    // XRack wireframe distance
    if TEST_COVERAGE {
        var surface = material.base.a * coverage_vertex_alpha(material.channels, vertex.alpha.x);
        if material.params.z > 0.5 && base_texture_uses_alpha(material.channels)
            && (material.params.w < 1.5 || material.params.x >= 0.0) {
            surface *= sample_base_texture(base_texture, base_sampler, material.base_uv, vertex.uv.xy).a;
        }
        let has_map = xr_has_opacity_map(material.channels.w);
        var opacity = xr_scalar_opacity(material.channels.w);
        let fades = has_map || opacity < 1.0;
        if has_map {
            opacity = xr_material_opacity(material.channels.w, material.opacity_uv, material.opacity_sampling,
                vertex.uv, dpdx(vertex.uv), dpdy(vertex.uv));
        }
        let coverage = material_coverage(material.params.w, surface, vertex.alpha.y, opacity, fades);
        if coverage.y < material.params.x || ((material.params.w > 0.5 || fades) && coverage.x <= 0.0) {
            discard;
        }
    }
    if LOD_FADE && !xr_lod_covered(vertex.metadata, vertex.position.xy) {
        discard;
    }
    // XRack wireframe discard
    if !TRACK_SURFACE || vertex.ids.x == 0u {
        // These surfaces cover the motion of any model behind them.
        return MotionOutput(vec4<f32>(0.0), 0u);
    }
    let current_depth = log2(max(vertex.depth.x, 1e-6));
    let previous = vertex.previous_clip;
    if vertex.ids.y == 0u || vertex.depth.y <= 0.0 || previous.w <= 0.0
        || previous.z < 0.0 || previous.z > previous.w {
        return MotionOutput(vec4<f32>(0.0, 0.0, -64.0, current_depth), vertex.ids.x);
    }
    let uv = previous.xy / previous.w * vec2<f32>(0.5, -0.5) + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return MotionOutput(vec4<f32>(0.0, 0.0, -64.0, current_depth), vertex.ids.x);
    }
    let motion = (uv - vertex.position.xy / cameras.extent.xy) * vertex.scale;
    // Larger displacements cannot sample this viewport. Keep them out of the f16 target.
    if any(abs(motion) > vec2<f32>(1.0)) {
        return MotionOutput(vec4<f32>(0.0, 0.0, -64.0, current_depth), vertex.ids.x);
    }
    return MotionOutput(vec4<f32>(motion, log2(vertex.depth.y), current_depth), vertex.ids.x);
}
