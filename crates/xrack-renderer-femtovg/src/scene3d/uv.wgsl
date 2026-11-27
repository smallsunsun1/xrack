struct TextureTransform {
    // First three lanes are the affine rows. W flags skip identity work.
    // Higher bits of v.w encode the portable anisotropy budget.
    u: vec4<f32>,
    v: vec4<f32>,
}

fn texture_uv(transform: TextureTransform, uv: vec2<f32>) -> vec2<f32> {
    if transform.u.w == 0.0 {
        return uv;
    }
    let point = vec3<f32>(uv, 1.0);
    return vec2<f32>(dot(transform.u.xyz, point), dot(transform.v.xyz, point));
}

fn texture_gradient(transform: TextureTransform, gradient: vec2<f32>) -> vec2<f32> {
    if transform.v.w == 0.0 {
        return gradient;
    }
    return vec2<f32>(dot(transform.u.xy, gradient), dot(transform.v.xy, gradient));
}


fn texture_anisotropy(transform: TextureTransform) -> u32 {
    return 1u + (u32(transform.v.w) >> 1u);
}

fn sample_base_texture(image: texture_2d<f32>, filtering: sampler,
                       transform: TextureTransform, uv: vec2<f32>) -> vec4<f32> {
    let at = texture_uv(transform, uv);
    if transform.v.w < 2.0 {
        return textureSample(image, filtering, at);
    }
    return xr_sample_texture_footprint(image, filtering, at,
        texture_gradient(transform, dpdx(uv)), texture_gradient(transform, dpdy(uv)),
        texture_anisotropy(transform));
}

fn base_texture_uses_alpha(channels: vec4<u32>) -> bool {
    // Scalar base-color selectors replicate a channel and use alpha one.
    return (channels.x & 15u) == 4u;
}

fn coverage_vertex_alpha(channels: vec4<u32>, vertex_alpha: f32) -> f32 {
    return select(vertex_alpha, 1.0, (channels.z & 0x80000000u) != 0u);
}

// Blend mode 2 takes opacity from the instance and tests the underlying
// material's mask separately. The other modes use the combined coverage.
fn material_coverage(mode: f32, surface_alpha: f32, tint_alpha: f32, opacity: f32, fades: bool) -> vec2<f32> {
    let coverage = surface_alpha * tint_alpha;
    let alpha = select(coverage, tint_alpha, mode > 1.5);
    let solid_fade = mode < 0.5 && fades;
    let blend_alpha = select(alpha, 1.0, solid_fade) * opacity;
    return vec2(blend_alpha, select(coverage, surface_alpha, mode > 1.5));
}

// Each scalar map keeps one cached linear channel in storage. A material can
// use all three alongside the full clearcoat layout at the sampled-texture limit.
struct ScalarPixels {
    dimensions: vec4<u32>,
    offsets: array<u32, 16>,
    words: array<u32>,
}

const OPACITY_MAP: u32 = 0u;
const METALNESS_MAP: u32 = 1u;
const ROUGHNESS_MAP: u32 = 2u;
const HEIGHT_MAP: u32 = 3u;
@group(1) @binding(34) var<storage, read> opacity_pixels: ScalarPixels;
@group(1) @binding(35) var<storage, read> metalness_pixels: ScalarPixels;
@group(1) @binding(36) var<storage, read> roughness_pixels: ScalarPixels;

fn xr_scalar_size(map: u32) -> vec2<u32> {
    switch map {
        case METALNESS_MAP: { return metalness_pixels.dimensions.xy; }
        case ROUGHNESS_MAP: { return roughness_pixels.dimensions.xy; }
        case HEIGHT_MAP: {
            let header = opacity_pixels.dimensions.z;
            if header == 0u { return opacity_pixels.dimensions.xy; }
            return vec2(opacity_pixels.words[header], opacity_pixels.words[header + 1u]);
        }
        default: { return opacity_pixels.dimensions.xy; }
    }
}

fn xr_scalar_offset(map: u32, level: u32) -> u32 {
    switch map {
        case METALNESS_MAP: { return metalness_pixels.offsets[level]; }
        case ROUGHNESS_MAP: { return roughness_pixels.offsets[level]; }
        case HEIGHT_MAP: {
            let header = opacity_pixels.dimensions.z;
            if header == 0u { return opacity_pixels.offsets[level]; }
            return opacity_pixels.words[header + 4u + level];
        }
        default: { return opacity_pixels.offsets[level]; }
    }
}

fn xr_scalar_word(map: u32, index: u32) -> u32 {
    switch map {
        case METALNESS_MAP: { return metalness_pixels.words[index]; }
        case ROUGHNESS_MAP: { return roughness_pixels.words[index]; }
        case HEIGHT_MAP: {
            let header = opacity_pixels.dimensions.z;
            let start = select(0u, header + 20u, header != 0u);
            return opacity_pixels.words[start + index];
        }
        default: { return opacity_pixels.words[index]; }
    }
}

fn xr_has_opacity_map(word: u32) -> bool {
    return (word & 0x80000000u) != 0u;
}

fn xr_scalar_opacity(word: u32) -> f32 {
    return bitcast<f32>(word & 0x7fffffffu);
}

fn xr_scalar_address(index: i32, size: i32, wrap: u32) -> u32 {
    if wrap == 0u {
        return u32(clamp(index, 0, size - 1));
    }
    let period = select(size, 2 * size, wrap == 2u);
    let wrapped = (index + period) % period;
    return u32(select(wrapped, min(wrapped, period - 1 - wrapped), wrap == 2u));
}

fn xr_scalar_texel(map: u32, at: vec2<i32>, size: vec2<u32>, wrap: vec2<u32>, offset: u32) -> f32 {
    let x = xr_scalar_address(at.x, i32(size.x), wrap.x);
    let y = xr_scalar_address(at.y, i32(size.y), wrap.y);
    let index = y * size.x + x;
    let word = xr_scalar_word(map, offset + (index >> 2u));
    return f32((word >> (8u * (index & 3u))) & 255u) / 255.0;
}

fn xr_scalar_coordinate(value: f32, wrap: u32) -> f32 {
    if wrap == 0u {
        return clamp(value, 0.0, 1.0);
    }
    let period = f32(wrap);
    return value - floor(value / period) * period;
}

fn xr_scalar_level(map: u32, size0: vec2<u32>, uv: vec2<f32>, level: u32,
                   flags: u32, linear: bool) -> f32 {
    let size = max(size0 >> vec2(level), vec2(1u));
    let wrap = vec2((flags >> 4u) & 3u, (flags >> 6u) & 3u);
    let offset = xr_scalar_offset(map, level);
    let point = vec2(xr_scalar_coordinate(uv.x, wrap.x), xr_scalar_coordinate(uv.y, wrap.y));
    var texel = point * vec2<f32>(size);
    if !linear {
        return xr_scalar_texel(map, vec2<i32>(floor(texel)), size, wrap, offset);
    }
    texel -= vec2(0.5);
    let base = vec2<i32>(floor(texel));
    let t = fract(texel);
    let top = mix(xr_scalar_texel(map, base, size, wrap, offset),
        xr_scalar_texel(map, base + vec2(1, 0), size, wrap, offset), t.x);
    let bottom = mix(xr_scalar_texel(map, base + vec2(0, 1), size, wrap, offset),
        xr_scalar_texel(map, base + vec2(1, 1), size, wrap, offset), t.x);
    return mix(top, bottom, t.y);
}

fn xr_scalar_filter(map: u32, size0: vec2<u32>, uv: vec2<f32>, footprint: f32, flags: u32, max_level: u32) -> f32 {
    if footprint <= 1.0 {
        return xr_scalar_level(map, size0, uv, 0u, flags, (flags & 2u) != 0u);
    }
    let lod = min(log2(footprint), f32(max_level));
    let linear = (flags & 1u) != 0u;
    if (flags & 4u) == 0u {
        return xr_scalar_level(map, size0, uv, u32(round(lod)), flags, linear);
    }
    let low = u32(floor(lod));
    let a = xr_scalar_level(map, size0, uv, low, flags, linear);
    if low == max_level {
        return a;
    }
    return mix(a, xr_scalar_level(map, size0, uv, low + 1u, flags, linear), fract(lod));
}

fn xr_sample_scalar_map(map: u32, transform: TextureTransform, sampling: vec4<u32>,
                        uv: vec4<f32>, dx: vec4<f32>, dy: vec4<f32>) -> f32 {
    let second = sampling.x != 0u;
    let point = texture_uv(transform, select(uv.xy, uv.zw, second));
    let u = texture_gradient(transform, select(dx.xy, dx.zw, second));
    let v = texture_gradient(transform, select(dy.xy, dy.zw, second));
    let size0 = xr_scalar_size(map);
    let size = vec2<f32>(size0);
    let u_pixels = length(u * size);
    let v_pixels = length(v * size);
    let major = max(u_pixels, v_pixels);
    let minor = min(u_pixels, v_pixels);
    let taps = clamp(u32(ceil(major / max(1.0, minor))), 1u, texture_anisotropy(transform));
    let footprint = max(minor, major / f32(taps));
    let axis = select(v, u, u_pixels >= v_pixels);
    var total = 0.0;
    for (var tap = 0u; tap < taps; tap += 1u) {
        let at = point + axis * ((f32(tap) + 0.5) / f32(taps) - 0.5);
        total += xr_scalar_filter(map, size0, at, footprint, sampling.y, sampling.z);
    }
    return total / f32(taps);
}

fn xr_material_opacity(word: u32, transform: TextureTransform, sampling: vec4<u32>,
                       uv: vec4<f32>, dx: vec4<f32>, dy: vec4<f32>) -> f32 {
    let scalar = xr_scalar_opacity(word);
    if !xr_has_opacity_map(word) {
        return scalar;
    }
    let sample = xr_sample_scalar_map(OPACITY_MAP, transform, sampling, uv, dx, dy);
    return scalar * select(sample, 1.0 - sample, (sampling.w & 2u) != 0u);
}
