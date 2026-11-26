// Keep the packed flags in sync with shader_texture_flags in material.rs.
fn xr_material_wrap_uv(uv: f32, mode: u32) -> f32 {
    switch mode {
        case 1u: { return fract(uv); }
        case 2u: { return 1.0 - abs(uv - 2.0 * floor(uv * 0.5) - 1.0); }
        default: { return clamp(uv, 0.0, 1.0); }
    }
}

fn xr_material_wrap_texel(texel: i32, size: i32, mode: u32) -> i32 {
    // After normalizing UV, linear samples extend by at most one texel.
    if mode == 1u {
        return (texel + size) % size;
    }
    return clamp(texel, 0, size - 1);
}

fn xr_material_address(texel: vec2<i32>, size: vec2<i32>, wrap: vec2<u32>) -> vec2<i32> {
    return vec2(xr_material_wrap_texel(texel.x, size.x, wrap.x),
        xr_material_wrap_texel(texel.y, size.y, wrap.y));
}

fn xr_material_wrap(flags: u32) -> vec2<u32> {
    return vec2((flags >> 2u) & 3u, (flags >> 4u) & 3u);
}

fn xr_material_sample_point(uv: vec2<f32>, size: vec2<i32>, wrap: vec2<u32>) -> vec2<f32> {
    return vec2(xr_material_wrap_uv(uv.x, wrap.x), xr_material_wrap_uv(uv.y, wrap.y))
        * vec2<f32>(size);
}

fn xr_material_world_position(image: texture_2d<f32>, uv: vec2<f32>, flags: u32,
                              inverse: mat4x4<f32>) -> XrCapturePosition {
    let size = vec2<i32>(textureDimensions(image));
    let wrap = xr_material_wrap(flags);
    let texel = vec2<i32>(xr_material_sample_point(uv, size, wrap));
    let pixel = xr_material_address(texel, size, wrap);
    return xr_capture_unproject(pixel, vec2<u32>(size), textureLoad(image, pixel, 0).x, inverse);
}

fn xr_material_load(image: texture_2d<f32>, texel: vec2<i32>, size: vec2<i32>,
                    wrap: vec2<u32>, level: i32) -> vec4<f32> {
    return textureLoad(image, xr_material_address(texel, size, wrap), level);
}

fn xr_material_sample_level(image: texture_2d<f32>, uv: vec2<f32>, flags: u32, level: i32) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(image, level));
    let wrap = xr_material_wrap(flags);
    let pixel = xr_material_sample_point(uv, size, wrap);
    if (flags & 2u) != 0u {
        let sample_point = pixel - vec2(0.5);
        let base = vec2<i32>(floor(sample_point));
        let fraction = fract(sample_point);
        let top = mix(xr_material_load(image, base, size, wrap, level),
            xr_material_load(image, base + vec2(1, 0), size, wrap, level), fraction.x);
        let bottom = mix(xr_material_load(image, base + vec2(0, 1), size, wrap, level),
            xr_material_load(image, base + vec2(1, 1), size, wrap, level), fraction.x);
        return mix(top, bottom, fraction.y);
    }
    return xr_material_load(image, vec2<i32>(pixel), size, wrap, level);
}

fn xr_material_decode(sampled: vec4<f32>, flags: u32) -> vec4<f32> {
    let encoding = (flags >> 6u) & 3u;
    if encoding == 0u {
        return sampled;
    }
    if encoding == 3u {
        return vec4(xr_capture_depth_value(sampled.x), sampled.yzw);
    }
    if sampled.a == 0.0 {
        return vec4(0.0);
    }
    let straight = sampled.rgb / sampled.a;
    if encoding == 2u {
        return vec4(srgb_to_linear(straight), sampled.a);
    }
    return vec4(straight, sampled.a);
}

fn xr_material_sample(image: texture_2d<f32>, uv: vec2<f32>, flags: u32) -> vec4<f32> {
    return xr_material_decode(xr_material_sample_level(image, uv, flags, 0), flags);
}

fn xr_material_sample_lod_raw(image: texture_2d<f32>, uv: vec2<f32>, flags: u32, lod: f32) -> vec4<f32> {
    let highest = i32(textureNumLevels(image)) - 1;
    let finite_lod = select(0.0, lod, abs(lod) <= 1e20);
    let level = clamp(finite_lod, 0.0, f32(highest));
    let texel_filter = select(flags & 2u, (flags >> 8u) & 2u, finite_lod > 0.0);
    let sample_flags = (flags & ~2u) | texel_filter;
    if level == 0.0 {
        return xr_material_sample_level(image, uv, sample_flags, 0);
    }
    if (flags & 256u) == 0u {
        return xr_material_sample_level(image, uv, sample_flags, i32(floor(level + 0.5)));
    }
    let lower = i32(floor(level));
    let fraction = fract(level);
    let a = xr_material_sample_level(image, uv, sample_flags, lower);
    if fraction == 0.0 {
        return a;
    }
    let b = xr_material_sample_level(image, uv, sample_flags, min(lower + 1, highest));
    return mix(a, b, fraction);
}

fn xr_material_sample_lod(image: texture_2d<f32>, uv: vec2<f32>, flags: u32, lod: f32) -> vec4<f32> {
    return xr_material_decode(xr_material_sample_lod_raw(image, uv, flags, lod), flags);
}

fn xr_material_sample_grad(image: texture_2d<f32>, uv: vec2<f32>, flags: u32,
                           dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    if !all(abs(vec4(dx, dy)) <= vec4(1e20)) {
        return xr_material_sample(image, uv, flags);
    }
    let size = vec2<f32>(textureDimensions(image));
    let maximum = 1u + ((flags >> 10u) & 15u);
    if maximum > 1u {
        let footprint = xr_anisotropic_footprint(dx, dy, size, textureNumLevels(image), maximum);
        if footprint.taps > 1u {
            var accumulated = vec4(0.0);
            for (var tap = 0u; tap < footprint.taps; tap += 1u) {
                accumulated += xr_material_sample_lod_raw(
                    image, xr_footprint_uv(footprint, uv, tap), flags, footprint.lod);
            }
            return xr_material_decode(accumulated / f32(footprint.taps), flags);
        }
    }
    let x = clamp(dx, vec2(-1e12), vec2(1e12)) * size;
    let y = clamp(dy, vec2(-1e12), vec2(1e12)) * size;
    let lod = 0.5 * log2(max(max(dot(x, x), dot(y, y)), 1e-20));
    return xr_material_sample_lod(image, uv, flags, lod);
}
