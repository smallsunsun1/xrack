struct Sky {
    // Face size at this level, mip, last mip, optional GGX sample budget.
    level: vec4<u32>,
    radiance: XrSkyRadiance,
    // Sample range and the mip's first texel in the FP32 accumulation buffer.
    slice: vec4<u32>,
}

@group(0) @binding(0) var<uniform> sky: Sky;
@group(0) @binding(1) var destination: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(2) var panorama: texture_cube<f32>;
@group(0) @binding(3) var filtering: sampler;

const TAU = 6.28318530718;
const PI = 3.14159265359;
const SKY_SAMPLES = 32u;
const SUN_SAMPLES = 16u;


fn tangent(direction: vec3<f32>) -> vec3<f32> {
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 0.0, 0.0), abs(direction.z) > 0.999);
    return normalize(cross(up, direction));
}

fn ggx_pdf(normal_half: f32, alpha_squared: f32) -> f32 {
    // The view equals the filtering direction, so N·H cancels in the
    // half-vector to reflected-direction Jacobian.
    let cos_squared = clamp(normal_half * normal_half, 0.0, 1.0);
    let denominator = (1.0 - cos_squared) + alpha_squared * cos_squared;
    return alpha_squared / (4.0 * PI * denominator * denominator);
}

fn horizon_weight(direction: vec3<f32>) -> f32 {
    return select(1.0, 1.0 - sky.radiance.sun_curve.z, direction.y < 0.0);
}

fn filtered(normal: vec3<f32>, roughness: f32) -> vec3<f32> {
    let x = tangent(normal);
    let y = cross(normal, x);
    let r2 = roughness * roughness;
    let a2 = max(r2 * r2, 0.000001);
    let solar = any(sky.radiance.sun_radiance.rgb > vec3<f32>(0.0));
    let cap = max(1.0 - sky.radiance.sun.w, 1e-7);
    let cap_pdf = f32(SUN_SAMPLES) / (f32(SKY_SAMPLES) * TAU * cap);
    var sky_sum = vec3<f32>(0.0);
    var sun_sum = 0.0;
    var weight = 0.0;

    for (var i = 0u; i < SKY_SAMPLES; i += 1u) {
        let phi = TAU * f32(i) / f32(SKY_SAMPLES);
        let v = f32(reverseBits(i)) * 2.32830643654e-10;
        let ct = sqrt((1.0 - v) / (1.0 + (a2 - 1.0) * v));
        let st = sqrt(max(0.0, 1.0 - ct * ct));
        let halfway = x * (cos(phi) * st) + y * (sin(phi) * st) + normal * ct;
        let l = reflect(-normal, halfway);
        let cosine = max(0.0, dot(normal, l));
        if cosine > 0.0 {
            let masked = cosine * horizon_weight(l);
            sky_sum += xr_sky_gradient(sky.radiance, l) * masked;
            weight += cosine;
            let sun_distance = dot(l, sky.radiance.sun.xyz);
            if solar && sun_distance > sky.radiance.sun.w {
                let pdf = ggx_pdf(ct, a2);
                sun_sum += xr_sky_sun_profile(sky.radiance, sun_distance) * masked * pdf / (pdf + cap_pdf);
            }
        }
    }
    if solar {
        // Direct cap samples keep small bright solar disks visible in rough
        // mips. Balance both sampling distributions in the overlapping area.
        let sun_x = tangent(sky.radiance.sun.xyz);
        let sun_y = cross(sky.radiance.sun.xyz, sun_x);
        for (var i = 0u; i < SUN_SAMPLES; i += 1u) {
            let t = 1.0 - cap * ((f32(i) + 0.5) / f32(SUN_SAMPLES));
            let phi = TAU * f32(reverseBits(i)) * 2.32830643654e-10;
            let ring = sqrt(max(0.0, 1.0 - t * t));
            let l = sun_x * (ring * cos(phi)) + sun_y * (ring * sin(phi)) + sky.radiance.sun.xyz * t;
            let cosine = dot(normal, l);
            if cosine > 0.0 {
                let pdf = ggx_pdf(dot(normal, normalize(normal + l)), a2);
                sun_sum += xr_sky_sun_profile(sky.radiance, t) * cosine * horizon_weight(l)
                    * pdf / (pdf + cap_pdf);
            }
        }
    }
    return (sky_sum + sun_sum * sky.radiance.sun_radiance.rgb) / max(weight, 1e-6);
}

@compute @workgroup_size(8, 8, 1)
fn generate(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(sky.level.x)) || id.z >= 6u {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) * (2.0 / f32(sky.level.x)) - 1.0;
    let direction = normalize(xr_cube_direction(id.z, uv.x, uv.y));
    var color: vec3<f32>;
    if sky.level.y == 0u {
        color = xr_sky_radiance(sky.radiance, direction) * horizon_weight(direction);
    } else {
        color = filtered(direction, f32(sky.level.y) / f32(sky.level.z));
    }
    textureStore(destination, vec2<i32>(id.xy), i32(id.z), vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(65504.0)), 1.0));
}

fn masked_panorama(direction: vec3<f32>) -> vec3<f32> {
    return textureSampleLevel(panorama, filtering, direction, 0.0).rgb * horizon_weight(direction);
}

fn mip_direction(id: vec3<u32>) -> vec3<f32> {
    let uv = (vec2<f32>(id.xy) + 0.5) * (2.0 / f32(sky.level.x)) - 1.0;
    return normalize(xr_cube_direction(id.z, uv.x, uv.y));
}

fn integrate_panorama(normal: vec3<f32>, range: vec2<u32>, initial: vec4<f32>) -> vec4<f32> {
    let roughness = f32(sky.level.y) / f32(sky.level.z);
    let x = tangent(normal);
    let y = cross(normal, x);
    let r2 = roughness * roughness;
    let a2 = max(r2 * r2, 0.000001);
    var accumulated = initial;
    let samples = select(64u, sky.level.w, sky.level.w != 0u);
    for (var i = range.x; i < range.y; i += 1u) {
        let phi = TAU * f32(i) / f32(samples);
        let v = f32(reverseBits(i)) * 2.32830643654e-10;
        let ct = sqrt((1.0 - v) / (1.0 + (a2 - 1.0) * v));
        let st = sqrt(max(0.0, 1.0 - ct * ct));
        let halfway = x * (cos(phi) * st) + y * (sin(phi) * st) + normal * ct;
        let direction = reflect(-normal, halfway);
        let cosine = dot(normal, direction);
        if cosine > 0.0 {
            accumulated += vec4(masked_panorama(direction) * cosine, cosine);
        }
    }
    return accumulated;
}

@compute @workgroup_size(8, 8, 1)
fn attenuate_panorama(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2(sky.level.x)) || id.z >= 6u {
        return;
    }
    let normal = mip_direction(id);
    var color: vec3<f32>;
    if sky.level.y == 0u {
        color = masked_panorama(normal);
    } else {
        let samples = select(64u, sky.level.w, sky.level.w != 0u);
        let total = integrate_panorama(normal, vec2(0u, samples), vec4(0.0));
        color = total.rgb / max(total.a, 1e-6);
    }
    textureStore(destination, vec2<i32>(id.xy), i32(id.z), vec4(color, 1.0));
}

@group(0) @binding(5) var<storage, read_write> prefilter_sums: array<vec4<f32>>;

fn prefilter_index(id: vec3<u32>) -> u32 {
    return sky.slice.z + (id.z * sky.level.x + id.y) * sky.level.x + id.x;
}

@compute @workgroup_size(8, 8, 1)
fn accumulate_panorama(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2(sky.level.x)) || id.z >= 6u {
        return;
    }
    let index = prefilter_index(id);
    var initial = vec4(0.0);
    if sky.slice.x != 0u {
        initial = prefilter_sums[index];
    }
    prefilter_sums[index] = integrate_panorama(mip_direction(id), sky.slice.xy, initial);
}

@compute @workgroup_size(8, 8, 1)
fn resolve_panorama(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2(sky.level.x)) || id.z >= 6u {
        return;
    }
    var color: vec3<f32>;
    if sky.level.y == 0u {
        color = masked_panorama(mip_direction(id));
    } else {
        let total = prefilter_sums[prefilter_index(id)];
        color = total.rgb / max(total.a, 1e-6);
    }
    textureStore(destination, vec2<i32>(id.xy), i32(id.z), vec4(color, 1.0));
}

@group(0) @binding(4) var<storage, read_write> irradiance_coefficients: array<vec4<f32>, 9>;
var<workgroup> irradiance_partial: array<vec3<f32>, 64>;

fn irradiance_basis(band: u32, d: vec3<f32>) -> f32 {
    switch band {
        case 0u: { return 0.282095; }
        case 1u: { return 0.488603 * d.y; }
        case 2u: { return 0.488603 * d.z; }
        case 3u: { return 0.488603 * d.x; }
        case 4u: { return 1.092548 * d.x * d.y; }
        case 5u: { return 1.092548 * d.y * d.z; }
        case 6u: { return 0.315392 * (3.0 * d.z * d.z - 1.0); }
        case 7u: { return 1.092548 * d.x * d.z; }
        default: { return 0.546274 * (d.x * d.x - d.y * d.y); }
    }
}

@compute @workgroup_size(64)
fn irradiance(@builtin(workgroup_id) group: vec3<u32>, @builtin(local_invocation_index) lane: u32) {
    const SIDE = 32u;
    const FACE = SIDE * SIDE;
    var coefficient = vec3(0.0);
    for (var pixel = lane; pixel < FACE * 6u; pixel += 64u) {
        let point = vec2(pixel % SIDE, (pixel % FACE) / SIDE);
        let uv = (vec2<f32>(point) + 0.5) * (2.0 / f32(SIDE)) - 1.0;
        let ray = xr_cube_direction(pixel / FACE, uv.x, uv.y);
        let inverse = inverseSqrt(dot(ray, ray));
        let direction = ray * inverse;
        let area = inverse * inverse * inverse * (4.0 / f32(FACE));
        coefficient += masked_panorama(direction) * (area * irradiance_basis(group.x, direction));
    }
    irradiance_partial[lane] = coefficient;
    workgroupBarrier();
    for (var step = 32u; step > 0u; step >>= 1u) {
        if lane < step {
            irradiance_partial[lane] += irradiance_partial[lane + step];
        }
        workgroupBarrier();
    }
    if lane == 0u {
        let convolution = select(select(0.25, 2.0 / 3.0, group.x < 4u), 1.0, group.x == 0u);
        irradiance_coefficients[group.x] = vec4(irradiance_partial[0] * convolution, 0.0);
    }
}
