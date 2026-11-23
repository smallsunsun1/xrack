struct SkinRows {
    x: vec4<f32>,
    y: vec4<f32>,
    z: vec4<f32>,
}

struct SkinWeights {
    @location(11) joints0: vec4<u32>,
    @location(12) joints1: vec4<u32>,
    @location(13) weights0: vec4<f32>,
    @location(14) weights1: vec4<f32>,
}

struct JointPalette {
    rows: array<SkinRows, 128>,
}

@group(3) @binding(0) var<uniform> skin_palette: JointPalette;

fn skin_transform(weights: SkinWeights) -> SkinRows {
    var result: SkinRows;
    for (var i = 0u; i < 4u; i += 1u) {
        let w0 = weights.weights0[i];
        if w0 > 0.0 {
            let joint = skin_palette.rows[weights.joints0[i]];
            result.x += joint.x * w0;
            result.y += joint.y * w0;
            result.z += joint.z * w0;
        }
        let w1 = weights.weights1[i];
        if w1 > 0.0 {
            let joint = skin_palette.rows[weights.joints1[i]];
            result.x += joint.x * w1;
            result.y += joint.y * w1;
            result.z += joint.z * w1;
        }
    }
    return result;
}

fn skin_point(skin: SkinRows, position: vec3<f32>) -> vec3<f32> {
    let p = vec4<f32>(position, 1.0);
    return vec3<f32>(dot(skin.x, p), dot(skin.y, p), dot(skin.z, p));
}

fn skin_columns(skin: SkinRows) -> mat3x3<f32> {
    return mat3x3<f32>(
        vec3<f32>(skin.x.x, skin.y.x, skin.z.x),
        vec3<f32>(skin.x.y, skin.y.y, skin.z.y),
        vec3<f32>(skin.x.z, skin.y.z, skin.z.z),
    );
}

fn skin_normal(linear: mat3x3<f32>, normal: vec3<f32>) -> vec3<f32> {
    let x = linear[0];
    let y = linear[1];
    let z = linear[2];
    let yz = cross(y, z);
    let cofactor = normal.x * yz + normal.y * cross(z, x) + normal.z * cross(x, y);
    if dot(cofactor, cofactor) <= 1e-24 {
        return normal;
    }
    return normalize(cofactor) * select(1.0, -1.0, dot(x, yz) < 0.0);
}
