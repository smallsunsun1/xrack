struct MorphSettings {
    geometry: vec4<u32>,
    weights: array<vec4<f32>, 8>,
}

struct MorphDisplacement {
    position: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec3<f32>,
    uvs: vec4<f32>,
    color: vec4<f32>,
    binormal: vec3<f32>,
}

const MORPH_FRAME: u32 = 1u;
const MORPH_COVERAGE: u32 = 2u;
const MORPH_SURFACE: u32 = 4u;
const MORPH_BINORMAL: u32 = 8u;

@group(3) @binding(1) var<uniform> morph_settings: MorphSettings;
@group(3) @binding(2) var<storage, read> morph_entries: array<u32>;

fn morph_vector(offset: u32) -> vec3<f32> {
    return vec3<f32>(
        bitcast<f32>(morph_entries[offset]),
        bitcast<f32>(morph_entries[offset + 1u]),
        bitcast<f32>(morph_entries[offset + 2u]),
    );
}

fn morph_vector4(offset: u32) -> vec4<f32> {
    return vec4<f32>(morph_vector(offset), bitcast<f32>(morph_entries[offset + 3u]));
}

fn morph_displacement(vertex: u32, channels: u32) -> MorphDisplacement {
    var delta: MorphDisplacement;
    let stride = morph_settings.geometry.y;
    let end = morph_entries[vertex + 1u];
    for (var entry = morph_entries[vertex]; entry < end; entry += 1u) {
        let offset = morph_settings.geometry.x + entry * stride;
        let target_index = morph_entries[offset];
        let weight = morph_settings.weights[target_index / 4u][target_index % 4u];
        if weight != 0.0 {
            delta.position += weight * morph_vector(offset + 1u);
            if (channels & MORPH_FRAME) != 0u {
                delta.normal += weight * morph_vector(offset + 4u);
                delta.tangent += weight * morph_vector(offset + 7u);
            }
            if stride > 10u {
                if (channels & MORPH_SURFACE) != 0u {
                    delta.uvs += weight * morph_vector4(offset + 10u);
                    delta.color += weight * morph_vector4(offset + 14u);
                } else if (channels & MORPH_COVERAGE) != 0u {
                    delta.uvs.x += weight * bitcast<f32>(morph_entries[offset + 10u]);
                    delta.uvs.y += weight * bitcast<f32>(morph_entries[offset + 11u]);
                    delta.color.a += weight * bitcast<f32>(morph_entries[offset + 17u]);
                }
                if (channels & MORPH_BINORMAL) != 0u {
                    delta.binormal += weight * morph_vector(offset + 18u);
                }
            }
        }
    }
    return delta;
}

fn morph_normal(initial: vec3<f32>, delta: vec3<f32>) -> vec3<f32> {
    let normal = initial + delta;
    if dot(normal, normal) <= 1e-24 {
        return initial;
    }
    return normalize(normal);
}
