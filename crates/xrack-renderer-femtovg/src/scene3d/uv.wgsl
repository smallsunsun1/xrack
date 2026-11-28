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

// Each binding holds a base image and optional companion images. The first
// image uses the inline headers and any later image starts at a word offset.
struct PackedPixels {
    dimensions: vec4<u32>,
    offsets: array<u32, 16>,
    words: array<u32>,
}

const OPACITY_MAP: u32 = 0u;
const METALNESS_MAP: u32 = 1u;
const ROUGHNESS_MAP: u32 = 2u;
const IRIDESCENCE_MAP: u32 = 3u;
const IRIDESCENCE_THICKNESS_MAP: u32 = 4u;
const SHEEN_COLOR_MAP: u32 = 5u;
const SHEEN_ROUGHNESS_MAP: u32 = 6u;
@group(1) @binding(34) var<storage, read> opacity_pixels: PackedPixels;
@group(1) @binding(35) var<storage, read> metalness_pixels: PackedPixels;
@group(1) @binding(36) var<storage, read> roughness_pixels: PackedPixels;

fn xr_packed_size(map: u32) -> vec2<u32> {
    switch map {
        case METALNESS_MAP: { return metalness_pixels.dimensions.xy; }
        case ROUGHNESS_MAP: { return roughness_pixels.dimensions.xy; }
        case IRIDESCENCE_MAP, SHEEN_COLOR_MAP: {
            let extra = select(metalness_pixels.dimensions.z, metalness_pixels.dimensions.w, map == SHEEN_COLOR_MAP);
            if extra == 0u { return metalness_pixels.dimensions.xy; }
            return vec2(metalness_pixels.words[extra], metalness_pixels.words[extra + 1u]);
        }
        case IRIDESCENCE_THICKNESS_MAP, SHEEN_ROUGHNESS_MAP: {
            let extra = select(roughness_pixels.dimensions.z, roughness_pixels.dimensions.w, map == SHEEN_ROUGHNESS_MAP);
            if extra == 0u { return roughness_pixels.dimensions.xy; }
            return vec2(roughness_pixels.words[extra], roughness_pixels.words[extra + 1u]);
        }
        default: { return opacity_pixels.dimensions.xy; }
    }
}

fn xr_packed_offset(map: u32, level: u32) -> u32 {
    switch map {
        case METALNESS_MAP: { return metalness_pixels.offsets[level]; }
        case ROUGHNESS_MAP: { return roughness_pixels.offsets[level]; }
        case IRIDESCENCE_MAP, SHEEN_COLOR_MAP: {
            let extra = select(metalness_pixels.dimensions.z, metalness_pixels.dimensions.w, map == SHEEN_COLOR_MAP);
            if extra == 0u { return metalness_pixels.offsets[level]; }
            return extra + 20u + metalness_pixels.words[extra + 4u + level];
        }
        case IRIDESCENCE_THICKNESS_MAP, SHEEN_ROUGHNESS_MAP: {
            let extra = select(roughness_pixels.dimensions.z, roughness_pixels.dimensions.w, map == SHEEN_ROUGHNESS_MAP);
            if extra == 0u { return roughness_pixels.offsets[level]; }
            return extra + 20u + roughness_pixels.words[extra + 4u + level];
        }
        default: { return opacity_pixels.offsets[level]; }
    }
}

fn xr_packed_word(map: u32, index: u32) -> u32 {
    switch map {
        case METALNESS_MAP, IRIDESCENCE_MAP, SHEEN_COLOR_MAP: { return metalness_pixels.words[index]; }
        case ROUGHNESS_MAP, IRIDESCENCE_THICKNESS_MAP, SHEEN_ROUGHNESS_MAP: { return roughness_pixels.words[index]; }
        default: { return opacity_pixels.words[index]; }
    }
}

fn srgb_to_linear(color: vec3<f32>) -> vec3<f32> {
    return select(color / 12.92, pow((color + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4)), color > vec3<f32>(0.04045));
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

fn xr_packed_texel(map: u32, at: vec2<i32>, size: vec2<u32>, wrap: vec2<u32>, offset: u32) -> vec3<f32> {
    let x = xr_scalar_address(at.x, i32(size.x), wrap.x);
    let y = xr_scalar_address(at.y, i32(size.y), wrap.y);
    let index = y * size.x + x;
    if map == SHEEN_COLOR_MAP {
        let byte_index = index * 3u;
        let word_index = offset + (byte_index >> 2u);
        let shift = 8u * (byte_index & 3u);
        var packed = xr_packed_word(map, word_index) >> shift;
        if shift > 8u {
            packed |= xr_packed_word(map, word_index + 1u) << (32u - shift);
        }
        // Convert individual texels before interpolation, just like an sRGB view.
        return srgb_to_linear(unpack4x8unorm(packed).rgb);
    }
    let word = xr_packed_word(map, offset + (index >> 2u));
    return vec3(f32((word >> (8u * (index & 3u))) & 255u) / 255.0, 0.0, 0.0);
}

fn xr_scalar_coordinate(value: f32, wrap: u32) -> f32 {
    if wrap == 0u {
        return clamp(value, 0.0, 1.0);
    }
    let period = f32(wrap);
    return value - floor(value / period) * period;
}

fn xr_packed_level(map: u32, size0: vec2<u32>, uv: vec2<f32>, level: u32,
                   flags: u32, linear: bool) -> vec3<f32> {
    let size = max(size0 >> vec2(level), vec2(1u));
    let wrap = vec2((flags >> 4u) & 3u, (flags >> 6u) & 3u);
    let offset = xr_packed_offset(map, level);
    let point = vec2(xr_scalar_coordinate(uv.x, wrap.x), xr_scalar_coordinate(uv.y, wrap.y));
    var texel = point * vec2<f32>(size);
    if !linear {
        return xr_packed_texel(map, vec2<i32>(floor(texel)), size, wrap, offset);
    }
    texel -= vec2(0.5);
    let base = vec2<i32>(floor(texel));
    let t = fract(texel);
    let top = mix(xr_packed_texel(map, base, size, wrap, offset),
        xr_packed_texel(map, base + vec2(1, 0), size, wrap, offset), t.x);
    let bottom = mix(xr_packed_texel(map, base + vec2(0, 1), size, wrap, offset),
        xr_packed_texel(map, base + vec2(1, 1), size, wrap, offset), t.x);
    return mix(top, bottom, t.y);
}

fn xr_packed_filter(map: u32, size0: vec2<u32>, uv: vec2<f32>, footprint: f32, flags: u32, max_level: u32) -> vec3<f32> {
    if footprint <= 1.0 {
        return xr_packed_level(map, size0, uv, 0u, flags, (flags & 2u) != 0u);
    }
    let lod = min(log2(footprint), f32(max_level));
    let linear = (flags & 1u) != 0u;
    if (flags & 4u) == 0u {
        return xr_packed_level(map, size0, uv, u32(round(lod)), flags, linear);
    }
    let low = u32(floor(lod));
    let a = xr_packed_level(map, size0, uv, low, flags, linear);
    if low == max_level {
        return a;
    }
    return mix(a, xr_packed_level(map, size0, uv, low + 1u, flags, linear), fract(lod));
}

fn xr_sample_packed_map(map: u32, transform: TextureTransform, sampling: vec4<u32>,
                        uv: vec4<f32>, dx: vec4<f32>, dy: vec4<f32>) -> vec3<f32> {
    let second = sampling.x != 0u;
    let point = texture_uv(transform, select(uv.xy, uv.zw, second));
    let u = texture_gradient(transform, select(dx.xy, dx.zw, second));
    let v = texture_gradient(transform, select(dy.xy, dy.zw, second));
    let size0 = xr_packed_size(map);
    let size = vec2<f32>(size0);
    let u_pixels = length(u * size);
    let v_pixels = length(v * size);
    let major = max(u_pixels, v_pixels);
    let minor = min(u_pixels, v_pixels);
    let taps = clamp(u32(ceil(major / max(1.0, minor))), 1u, texture_anisotropy(transform));
    let footprint = max(minor, major / f32(taps));
    let axis = select(v, u, u_pixels >= v_pixels);
    var total = vec3(0.0);
    for (var tap = 0u; tap < taps; tap += 1u) {
        let at = point + axis * ((f32(tap) + 0.5) / f32(taps) - 0.5);
        total += xr_packed_filter(map, size0, at, footprint, sampling.y, sampling.z);
    }
    return total / f32(taps);
}

fn xr_sample_scalar_map(map: u32, transform: TextureTransform, sampling: vec4<u32>,
                        uv: vec4<f32>, dx: vec4<f32>, dy: vec4<f32>) -> f32 {
    return xr_sample_packed_map(map, transform, sampling, uv, dx, dy).x;
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

// Resolve the footprint, mip offsets and address modes once per view ray.
// The ray samples only this part of the shared opacity/height binding.
struct HeightMip {
    size: vec2<i32>,
    offset: u32,
}

struct HeightFilter {
    first: HeightMip,
    second: HeightMip,
    blend: f32,
    linear: bool,
    wrap: vec2<u32>,
}

fn xr_height_header() -> vec4<u32> {
    let start = opacity_pixels.dimensions.z;
    if start == 0u {
        return opacity_pixels.dimensions;
    }
    return vec4(opacity_pixels.words[start], opacity_pixels.words[start + 1u],
        start, opacity_pixels.words[start + 3u]);
}

fn xr_height_mip(header: vec4<u32>, level: u32) -> HeightMip {
    let size = vec2<i32>(max(header.xy >> vec2(level), vec2(1u)));
    if header.z == 0u {
        return HeightMip(size, opacity_pixels.offsets[level]);
    }
    return HeightMip(size, header.z + 20u + opacity_pixels.words[header.z + 4u + level]);
}

fn xr_height_filter(header: vec4<u32>, footprint: f32, flags: u32, max_level: u32) -> HeightFilter {
    let wrap = vec2((flags >> 4u) & 3u, (flags >> 6u) & 3u);
    if footprint <= 1.0 {
        let mip = xr_height_mip(header, 0u);
        return HeightFilter(mip, mip, 0.0, (flags & 2u) != 0u, wrap);
    }
    let lod = min(log2(footprint), f32(max_level));
    let linear = (flags & 1u) != 0u;
    if (flags & 4u) == 0u {
        let mip = xr_height_mip(header, u32(round(lod)));
        return HeightFilter(mip, mip, 0.0, linear, wrap);
    }
    let low = u32(floor(lod));
    return HeightFilter(xr_height_mip(header, low), xr_height_mip(header, min(low + 1u, max_level)),
        fract(lod), linear, wrap);
}

fn xr_height_adjacent(index: i32, size: i32, wrap: u32) -> vec2<u32> {
    if wrap == 0u {
        return vec2<u32>(clamp(vec2(index, index + 1), vec2(0), vec2(size - 1)));
    }
    let period = select(size, 2 * size, wrap == 2u);
    let first = (index + period) % period;
    let indices = vec2(first, select(first + 1, 0, first + 1 == period));
    return vec2<u32>(select(indices, min(indices, vec2(period - 1) - indices), wrap == 2u));
}

fn xr_height_texel(index: u32, offset: u32) -> f32 {
    let word = opacity_pixels.words[offset + (index >> 2u)];
    return f32((word >> (8u * (index & 3u))) & 255u) / 255.0;
}

fn xr_height_level(mip: HeightMip, at: vec2<f32>, wrap: vec2<u32>, linear: bool) -> f32 {
    let point = at * vec2<f32>(mip.size);
    if !linear {
        let base = vec2<i32>(floor(point));
        let x = xr_scalar_address(base.x, mip.size.x, wrap.x);
        let y = xr_scalar_address(base.y, mip.size.y, wrap.y);
        return xr_height_texel(y * u32(mip.size.x) + x, mip.offset);
    }
    let texel = point - vec2(0.5);
    let base = vec2<i32>(floor(texel));
    let t = fract(texel);
    let x = xr_height_adjacent(base.x, mip.size.x, wrap.x);
    let row = xr_height_adjacent(base.y, mip.size.y, wrap.y) * u32(mip.size.x);
    let top = mix(xr_height_texel(row.x + x.x, mip.offset), xr_height_texel(row.x + x.y, mip.offset), t.x);
    let bottom = mix(xr_height_texel(row.y + x.x, mip.offset), xr_height_texel(row.y + x.y, mip.offset), t.x);
    return mix(top, bottom, t.y);
}

fn xr_height_sample(filtering: HeightFilter, uv: vec2<f32>) -> f32 {
    let point = vec2(xr_scalar_coordinate(uv.x, filtering.wrap.x), xr_scalar_coordinate(uv.y, filtering.wrap.y));
    let first = xr_height_level(filtering.first, point, filtering.wrap, filtering.linear);
    if filtering.blend == 0.0 {
        return first;
    }
    return mix(first, xr_height_level(filtering.second, point, filtering.wrap, filtering.linear), filtering.blend);
}
