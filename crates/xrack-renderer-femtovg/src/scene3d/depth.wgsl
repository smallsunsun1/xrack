override DEPTH_ALPHA: u32 = 0u;

struct Projection {
    x: vec4<f32>,
    y: vec4<f32>,
    z: vec4<f32>,
    w: vec4<f32>,
}

struct Material {
    base: vec4<f32>,
    params: vec4<f32>,
    base_uv: TextureTransform,
    channels: vec4<u32>,
    shader_parameters: vec4<f32>,
}

@group(0) @binding(0) var<uniform> clip: Projection;
@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var base_sampler: sampler;

struct Geometry {
    @location(0) position: vec3<f32>,
    @location(4) x: vec4<f32>,
    @location(5) y: vec4<f32>,
    @location(6) z: vec4<f32>,
    @location(7) tint: vec4<f32>,
    @location(8) metadata: u32,
    @location(15) instance_data: vec4<f32>,
}

fn project(vertex: Geometry) -> vec4<f32> {
    let local = vec4(xr_displaced_vertex(vertex.position, material.shader_parameters, vertex.instance_data,
        vertex.metadata >> 3u, vertex.tint, mat3x4(vertex.x, vertex.y, vertex.z)), 1.0);
    let world = vec4<f32>(dot(vertex.x, local), dot(vertex.y, local), dot(vertex.z, local), 1.0);
    return vec4<f32>(dot(clip.x, world), dot(clip.y, world), dot(clip.z, world), dot(clip.w, world));
}

@vertex
fn vertex_opaque(vertex: Geometry) -> @builtin(position) vec4<f32> {
    return project(vertex);
}

struct MaskVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: vec2<f32>,
}

@vertex
fn vertex_mask(vertex: Geometry, @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>) -> MaskVertex {
    return MaskVertex(project(vertex), uv, vec2<f32>(color.a, vertex.tint.a));
}

// Instance alpha also covers particles hidden by an affector or lifetime fade.
fn particle_shadow_position(vertex: Geometry, alpha: f32) -> vec4<f32> {
    if alpha < 0.5 {
        return vec4<f32>(0.0, 0.0, 2.0, 1.0);
    }
    return project(vertex);
}

@vertex
fn vertex_particle_opaque(vertex: Geometry) -> @builtin(position) vec4<f32> {
    return particle_shadow_position(vertex, vertex.tint.a);
}

@vertex
fn vertex_particle_mask(vertex: Geometry, @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>) -> MaskVertex {
    return MaskVertex(particle_shadow_position(vertex, vertex.tint.a), uv, vec2<f32>(color.a, 1.0));
}

@vertex
fn vertex_skin_opaque(vertex: Geometry, weights: SkinWeights) -> @builtin(position) vec4<f32> {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position);
    return project(deformed);
}

@vertex
fn vertex_skin_mask(vertex: Geometry, weights: SkinWeights, @location(2) uv: vec2<f32>,
                    @location(3) color: vec4<f32>) -> MaskVertex {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position);
    return MaskVertex(project(deformed), uv, vec2<f32>(color.a, vertex.tint.a));
}

@vertex
fn vertex_morph_opaque(vertex: Geometry, @builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var deformed = vertex;
    deformed.position += morph_displacement(index).position;
    return project(deformed);
}

@vertex
fn vertex_morph_mask(vertex: Geometry, @builtin(vertex_index) index: u32, @location(2) uv: vec2<f32>,
                     @location(3) color: vec4<f32>) -> MaskVertex {
    var deformed = vertex;
    deformed.position += morph_displacement(index).position;
    return MaskVertex(project(deformed), uv, vec2<f32>(color.a, vertex.tint.a));
}

@vertex
fn vertex_skin_morph_opaque(vertex: Geometry, weights: SkinWeights,
                            @builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position + morph_displacement(index).position);
    return project(deformed);
}

@vertex
fn vertex_skin_morph_mask(vertex: Geometry, weights: SkinWeights, @builtin(vertex_index) index: u32,
                          @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>) -> MaskVertex {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position + morph_displacement(index).position);
    return MaskVertex(project(deformed), uv, vec2<f32>(color.a, vertex.tint.a));
}

@fragment
fn fragment_mask(vertex: MaskVertex) {
    var surface = material.base.a * coverage_vertex_alpha(material.channels, vertex.alpha.x);
    if material.params.z > 0.5 && base_texture_uses_alpha(material.channels)
        && (material.params.w < 1.5 || material.params.x >= 0.0) {
        surface *= textureSample(base_texture, base_sampler, texture_uv(material.base_uv, vertex.uv)).a;
    }
    let coverage = material_coverage(material.params.w, surface, vertex.alpha.y);
    if coverage.y < material.params.x {
        discard;
    }
    if DEPTH_ALPHA == 1u {
        if coverage.x <= 0.0 {
            discard;
        }
    } else if DEPTH_ALPHA == 2u {
        if coverage.x < 1.0 {
            discard;
        }
    }
}
