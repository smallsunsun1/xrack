struct Settings {
    face: u32,
    size: u32,
    roughness: f32,
    padding: u32,
}

@group(0) @binding(0) var radiance: texture_cube<f32>;
@group(0) @binding(1) var seamless: sampler;
@group(0) @binding(2) var<uniform> settings: Settings;

const SAMPLE_COUNT = 64u;

@vertex
fn vertex(@builtin(vertex_index) id: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}


@fragment
fn fragment(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = point.xy * (2.0 / f32(settings.size)) - 1.0;
    let n = normalize(xr_cube_direction(settings.face, uv.x, uv.y));
    let axis = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(n.z) > 0.999);
    let x = normalize(cross(axis, n));
    let y = cross(n, x);
    let alpha = settings.roughness * settings.roughness;
    var sum = vec3<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < SAMPLE_COUNT; i += 1u) {
        let phi = 6.28318530718 * f32(i) / f32(SAMPLE_COUNT);
        let v = f32(reverseBits(i)) * 2.32830643654e-10;
        let cos_theta = sqrt((1.0 - v) / (1.0 + (alpha * alpha - 1.0) * v));
        let sin_theta = sqrt(max(0.0, 1.0 - cos_theta * cos_theta));
        let h = x * (cos(phi) * sin_theta) + y * (sin(phi) * sin_theta) + n * cos_theta;
        let direction = reflect(-n, h);
        let w = max(dot(n, direction), 0.0);
        if w > 0.0 {
            sum += textureSampleLevel(radiance, seamless, direction, 0.0).rgb * w;
            weight += w;
        }
    }
    return vec4<f32>(sum / max(weight, 1e-6), 1.0);
}
