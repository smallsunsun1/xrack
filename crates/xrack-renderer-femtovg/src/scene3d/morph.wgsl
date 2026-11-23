struct MorphSettings {
    geometry: vec4<u32>,
    weights: array<vec4<f32>, 8>,
}

struct MorphDisplacement {
    position: vec3<f32>,
    normal: vec3<f32>,
    tangent: vec3<f32>,
}

@group(3) @binding(1) var<uniform> morph_settings: MorphSettings;
@group(3) @binding(2) var<storage, read> morph_entries: array<u32>;

fn morph_vector(offset: u32) -> vec3<f32> {
    return vec3<f32>(
        bitcast<f32>(morph_entries[offset]),
        bitcast<f32>(morph_entries[offset + 1u]),
        bitcast<f32>(morph_entries[offset + 2u]),
    );
}

fn morph_displacement(vertex: u32) -> MorphDisplacement {
    var delta: MorphDisplacement;
    let end = morph_entries[vertex + 1u];
    for (var entry = morph_entries[vertex]; entry < end; entry += 1u) {
        let offset = morph_settings.geometry.x + entry * 10u;
        let target_index = morph_entries[offset];
        let weight = morph_settings.weights[target_index / 4u][target_index % 4u];
        if weight != 0.0 {
            delta.position += weight * morph_vector(offset + 1u);
            delta.normal += weight * morph_vector(offset + 4u);
            delta.tangent += weight * morph_vector(offset + 7u);
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
