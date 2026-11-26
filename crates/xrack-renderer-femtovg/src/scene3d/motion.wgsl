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
    @location(15) instance_data: vec4<f32>,
}

struct RasterVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) previous_clip: vec4<f32>,
    @location(1) depth: vec2<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) alpha: vec2<f32>,
    @location(4) @interpolate(flat) ids: vec2<u32>,
    @location(5) @interpolate(flat) scale: f32,
    @location(6) @interpolate(flat) metadata: u32,
    // XRack wireframe varying
}

fn paired_vertex(vertex: Vertex, index: u32, current: vec3<f32>, previous: vec3<f32>) -> RasterVertex {
    let old = previous_instances[index];
    let p = vec4(xr_displaced_vertex(current, material.shader_parameters, vertex.instance_data,
        xr_instance_number(vertex.metadata.x), vertex.tint, mat3x4(vertex.x, vertex.y, vertex.z)), 1.0);
    let old_tint = vec4<f32>((vec4<u32>(old.tint) >> vec4<u32>(24u, 16u, 8u, 0u)) & vec4<u32>(255u)) / 255.0;
    let q = vec4(xr_displaced_vertex(previous, previous_vertex_parameters, old.instance_data,
        old.index_and_enabled >> 1u, old_tint, mat3x4(old.x, old.y, old.z)), 1.0);
    let world = vec4<f32>(dot(vertex.x, p), dot(vertex.y, p), dot(vertex.z, p), 1.0);
    let previous_world = vec4<f32>(dot(old.x, q), dot(old.y, q), dot(old.z, q), 1.0);
    return RasterVertex(
        world * cameras.current.clip,
        previous_world * cameras.previous.clip,
        vec2<f32>(dot(cameras.current.depth, world), dot(cameras.previous.depth, previous_world)),
        vertex.uv,
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

fn previous_morph_point(index: u32, point: vec3<f32>) -> vec3<f32> {
    var result = point;
    let end = morph_entries[index + 1u];
    for (var entry = morph_entries[index]; entry < end; entry += 1u) {
        let offset = previous_morph.geometry.x + entry * 10u;
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

@vertex
fn morphed(vertex: Vertex, @builtin(instance_index) instance: u32) -> RasterVertex {
    return paired_vertex(vertex, instance,
        vertex.point + morph_displacement(vertex.index).position, previous_morph_point(vertex.index, vertex.point));
}

@vertex
fn skinned_morphed(vertex: Vertex, weights: SkinWeights,
    @builtin(instance_index) instance: u32) -> RasterVertex {
    return paired_vertex(vertex, instance,
        skin_point(skin_transform(weights), vertex.point + morph_displacement(vertex.index).position),
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
            surface *= sample_base_texture(base_texture, base_sampler, material.base_uv, vertex.uv).a;
        }
        let coverage = material_coverage(material.params.w, surface, vertex.alpha.y);
        if coverage.y < material.params.x || (material.params.w > 0.5 && coverage.x <= 0.0) {
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
