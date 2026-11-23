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
}

fn project(vertex: Geometry) -> vec4<f32> {
    let local = vec4<f32>(vertex.position, 1.0);
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
    @location(1) alpha: f32,
}

@vertex
fn vertex_mask(vertex: Geometry, @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>,
               @location(7) tint: vec4<f32>) -> MaskVertex {
    return MaskVertex(project(vertex), uv, color.a * tint.a);
}

@vertex
fn vertex_skin_opaque(vertex: Geometry, weights: SkinWeights) -> @builtin(position) vec4<f32> {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position);
    return project(deformed);
}

@vertex
fn vertex_skin_mask(vertex: Geometry, weights: SkinWeights, @location(2) uv: vec2<f32>,
                    @location(3) color: vec4<f32>, @location(7) tint: vec4<f32>) -> MaskVertex {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position);
    return MaskVertex(project(deformed), uv, color.a * tint.a);
}

@vertex
fn vertex_morph_opaque(vertex: Geometry, @builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var deformed = vertex;
    deformed.position += morph_displacement(index).position;
    return project(deformed);
}

@vertex
fn vertex_morph_mask(vertex: Geometry, @builtin(vertex_index) index: u32, @location(2) uv: vec2<f32>,
                     @location(3) color: vec4<f32>, @location(7) tint: vec4<f32>) -> MaskVertex {
    var deformed = vertex;
    deformed.position += morph_displacement(index).position;
    return MaskVertex(project(deformed), uv, color.a * tint.a);
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
                          @location(2) uv: vec2<f32>, @location(3) color: vec4<f32>,
                          @location(7) tint: vec4<f32>) -> MaskVertex {
    var deformed = vertex;
    deformed.position = skin_point(skin_transform(weights), vertex.position + morph_displacement(index).position);
    return MaskVertex(project(deformed), uv, color.a * tint.a);
}

@fragment
fn fragment_mask(vertex: MaskVertex) {
    var alpha = material.base.a * vertex.alpha;
    if material.params.z > 0.5 {
        alpha *= textureSample(base_texture, base_sampler, texture_uv(material.base_uv, vertex.uv)).a;
    }
    if alpha < material.params.x {
        discard;
    }
}
