struct Sky {
    // Face size at this level, mip, last mip.
    level: vec4<u32>,
    radiance: XrSkyRadiance,
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

@compute @workgroup_size(8, 8, 1)
fn attenuate_panorama(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= vec2<u32>(sky.level.x)) || id.z >= 6u {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) * (2.0 / f32(sky.level.x)) - 1.0;
    let normal = normalize(xr_cube_direction(id.z, uv.x, uv.y));
    var color = vec3<f32>(0.0);
    if sky.level.y == 0u {
        color = masked_panorama(normal);
    } else {
        let roughness = f32(sky.level.y) / f32(sky.level.z);
        let x = tangent(normal);
        let y = cross(normal, x);
        let r2 = roughness * roughness;
        let a2 = max(r2 * r2, 0.000001);
        var weight = 0.0;
        for (var i = 0u; i < 64u; i += 1u) {
            let phi = TAU * f32(i) / 64.0;
            let v = f32(reverseBits(i)) * 2.32830643654e-10;
            let ct = sqrt((1.0 - v) / (1.0 + (a2 - 1.0) * v));
            let st = sqrt(max(0.0, 1.0 - ct * ct));
            let halfway = x * (cos(phi) * st) + y * (sin(phi) * st) + normal * ct;
            let direction = reflect(-normal, halfway);
            let cosine = dot(normal, direction);
            if cosine > 0.0 {
                color += masked_panorama(direction) * cosine;
                weight += cosine;
            }
        }
        color /= max(weight, 1e-6);
    }
    textureStore(destination, vec2<i32>(id.xy), i32(id.z), vec4<f32>(color, 1.0));
}
