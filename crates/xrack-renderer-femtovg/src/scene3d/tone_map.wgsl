override COMPOSITE_ALPHA: bool = false;
override LINEAR_OUTPUT: bool = false;
override PACKED_SIGNAL: bool = false;

struct Settings {
    // Exposure, bloom strength, SceneToneMap discriminant, filmic white point.
    tone: vec4<f32>,
    // Brightness, contrast, saturation, adjustments enabled.
    grade: vec4<f32>,
    // LUT lattice size and blend amount, then output width and height.
    lut: vec4<f32>,
    // Linear tint, followed by the vignette amount.
    vignette: vec4<f32>,
    // Inner radius, sharpening amount, dithering enabled, inverse half-diagonal.
    finish: vec4<f32>,
    // Lens flare gain, applied to the separately rendered low-resolution image.
    flare: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: Settings;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var bloom_sampler: sampler;
@group(0) @binding(4) var lookup_table: texture_2d<f32>;
@group(0) @binding(5) var lookup_sampler: sampler;
@group(0) @binding(6) var source_sampler: sampler;
@group(0) @binding(7) var lens_flare: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let position = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    return vec4<f32>(position, 0.0, 1.0);
}

fn filmic(x: vec3<f32>) -> vec3<f32> {
    // Hable's filmic operator, normalized separately to the selected white point.
    let shoulder = 0.15;
    let linear = 0.50;
    let toe = 0.20;
    return ((x * (shoulder * x + 0.10 * linear) + toe * 0.02)
        / (x * (shoulder * x + linear) + toe * 0.30)) - 0.02 / 0.30;
}

fn map_radiance(radiance: vec3<f32>) -> vec3<f32> {
    let mode = u32(settings.tone.z);
    if mode == 0u {
        return radiance;
    }
    let x = max(vec3<f32>(0.0), radiance * settings.tone.x);
    switch mode {
        case 1u: { return x; }
        case 3u: {
            // Narkowicz's compact ACES-inspired fit.
            return x * (2.51 * x + 0.03) / (x * (2.43 * x + 0.59) + 0.14);
        }
        case 4u: {
            let toe = max(x - 0.004, vec3<f32>(0.0));
            let encoded = toe * (6.2 * toe + 0.5) / (toe * (6.2 * toe + 1.7) + 0.06);
            // The operator approximates display gamma. Decode before sRGB output.
            return pow(encoded, vec3<f32>(2.2));
        }
        case 5u: {
            return filmic(x) / max(filmic(vec3<f32>(settings.tone.w)), vec3<f32>(0.00001));
        }
        default: {
            // Use a shared denominator so bright colors keep their ratios.
            return x / (1.0 + max(x.r, max(x.g, x.b)));
        }
    }
}

fn display_color(radiance: vec3<f32>) -> vec4<f32> {
    var color = map_radiance(radiance);
    if !LINEAR_OUTPUT {
        color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    if settings.grade.w > 0.5 {
        color *= settings.grade.x;
        color = vec3<f32>(0.18) + (color - vec3<f32>(0.18)) * settings.grade.y;
        let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
        color = mix(vec3<f32>(luminance), color, settings.grade.z);
    }
    if !LINEAR_OUTPUT {
        color = clamp(color, vec3<f32>(0.0), vec3<f32>(1.0));
    }
    return vec4<f32>(color, 1.0);
}

fn lut_color(color: vec4<f32>) -> vec4<f32> {
    let size = settings.lut.x;
    let signal = encode_srgb(color.rgb);
    let position = clamp(signal, vec3<f32>(0.0), vec3<f32>(1.0)) * (size - 1.0);
    let lower = floor(position.b);
    let upper = min(lower + 1.0, size - 1.0);
    let dimensions = vec2<f32>(size * size, size);
    let at = position.rg + vec2<f32>(0.5);
    let a = textureSampleLevel(lookup_table, lookup_sampler,
        (at + vec2<f32>(lower * size, 0.0)) / dimensions, 0.0).rgb;
    let b = textureSampleLevel(lookup_table, lookup_sampler,
        (at + vec2<f32>(upper * size, 0.0)) / dimensions, 0.0).rgb;
    let graded = mix(a, b, position.b - lower);
    return vec4<f32>(decode_srgb(mix(signal, graded, settings.lut.y)), color.a);
}

fn display_sample(sample: vec4<f32>, glow: vec3<f32>) -> vec4<f32> {
    let radiance = sample.rgb + glow;
    if !COMPOSITE_ALPHA || sample.a >= 1.0 {
        return display_color(radiance);
    }
    // A glow can extend into pixels with no geometry. Give it coverage
    // proportional to its display luminance so it can composite over 2D.
    let halo = encode_srgb(clamp(map_radiance(glow), vec3<f32>(0.0), vec3<f32>(1.0)));
    let alpha = max(sample.a, max(halo.r, max(halo.g, halo.b)));
    if alpha <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(display_color(radiance / alpha).rgb, alpha);
}

fn encode_composite(source_color: vec4<f32>, position: vec2<f32>) -> vec4<f32> {
    var color = source_color;
    if settings.vignette.a > 0.0 {
        let radial = length(position - settings.lut.zw * 0.5) * settings.finish.w;
        let amount = smoothstep(settings.finish.x, 1.0, radial) * settings.vignette.a;
        color = vec4<f32>(mix(color.rgb, settings.vignette.rgb, amount), color.a);
    }
    if PACKED_SIGNAL {
        return vec4<f32>(encode_srgb(color.rgb) * color.a, color.a);
    }
    if LINEAR_OUTPUT {
        return vec4<f32>(color.rgb * color.a, color.a);
    }
    if settings.finish.z > 0.5 {
        // Dither the bytes consumed by the compositor, including premultiplied alpha.
        let encoded = dither_display_rgb(encode_srgb(color.rgb) * color.a, color.a, position, true);
        return vec4<f32>(decode_srgb(encoded), color.a);
    }
    if !COMPOSITE_ALPHA || color.a >= 1.0 {
        return color;
    }
    // The sRGB render attachment encodes RGB on write. FemtoVG reads these
    // bytes through an unorm view and expects premultiplied display color.
    return vec4<f32>(decode_srgb(encode_srgb(color.rgb) * color.a), color.a);
}

fn read_scene(position: vec2<f32>, exact: bool) -> vec4<f32> {
    if exact {
        return textureLoad(source, vec2<i32>(position), 0);
    }
    return textureSampleLevel(source, source_sampler, position / settings.lut.zw, 0.0);
}

fn scene_sample(position: vec2<f32>) -> vec4<f32> {
    let exact = all(textureDimensions(source) == vec2<u32>(settings.lut.zw));
    let center = read_scene(position, exact);
    if settings.finish.y <= 0.0 || center.a <= 0.0 {
        return center;
    }
    let first = vec2<f32>(0.5);
    let last = settings.lut.zw - first;
    let x = vec2<f32>(1.0, 0.0);
    let y = vec2<f32>(0.0, 1.0);
    let neighbors = read_scene(max(first, position - x), exact)
        + read_scene(min(last, position + x), exact)
        + read_scene(max(first, position - y), exact)
        + read_scene(min(last, position + y), exact);
    if neighbors.a <= 0.00001 {
        return center;
    }
    // Normalize by neighboring coverage so transparent pixels do not cause dark halos.
    let average = neighbors.rgb * (center.a / neighbors.a);
    let sharp = max(vec3<f32>(0.0), center.rgb + settings.finish.y * (center.rgb - average));
    return vec4<f32>(sharp, center.a);
}

fn bloom_glow(position: vec4<f32>) -> vec3<f32> {
    let uv = position.xy / settings.lut.zw;
    return textureSampleLevel(bloom, bloom_sampler, uv, 0.0).rgb * settings.tone.y;
}

fn flare_glow(position: vec4<f32>) -> vec3<f32> {
    let uv = position.xy / settings.lut.zw;
    return textureSampleLevel(lens_flare, source_sampler, uv, 0.0).rgb * settings.flare.x;
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_composite(display_sample(scene_sample(position.xy), vec3<f32>(0.0)), position.xy);
}

@fragment
fn fragment_bloom(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_composite(display_sample(scene_sample(position.xy), bloom_glow(position)), position.xy);
}

@fragment
fn fragment_lut(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_composite(lut_color(display_sample(scene_sample(position.xy), vec3<f32>(0.0))), position.xy);
}

@fragment
fn fragment_bloom_lut(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_composite(lut_color(display_sample(scene_sample(position.xy), bloom_glow(position))), position.xy);
}

@fragment
fn fragment_flare(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_composite(display_sample(scene_sample(position.xy), flare_glow(position)), position.xy);
}

@fragment
fn fragment_bloom_flare(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let glow = bloom_glow(position) + flare_glow(position);
    return encode_composite(display_sample(scene_sample(position.xy), glow), position.xy);
}

@fragment
fn fragment_flare_lut(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return encode_composite(lut_color(display_sample(scene_sample(position.xy), flare_glow(position))), position.xy);
}

@fragment
fn fragment_bloom_flare_lut(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let glow = bloom_glow(position) + flare_glow(position);
    return encode_composite(lut_color(display_sample(scene_sample(position.xy), glow)), position.xy);
}
