struct Settings {
    // Exposure, bloom strength, SceneToneMap discriminant, filmic white point.
    tone: vec4<f32>,
    // Brightness, contrast, saturation, adjustments enabled.
    grade: vec4<f32>,
    // LUT lattice size and blend amount.
    lut: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: Settings;
@group(0) @binding(2) var bloom: texture_2d<f32>;
@group(0) @binding(3) var bloom_sampler: sampler;
@group(0) @binding(4) var lookup_table: texture_2d<f32>;
@group(0) @binding(5) var lookup_sampler: sampler;

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
    var color = clamp(map_radiance(radiance), vec3<f32>(0.0), vec3<f32>(1.0));
    if settings.grade.w > 0.5 {
        color *= settings.grade.x;
        color = vec3<f32>(0.18) + (color - vec3<f32>(0.18)) * settings.grade.y;
        let luminance = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
        color = mix(vec3<f32>(luminance), color, settings.grade.z);
    }
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055,
                  linear * 12.92, linear <= vec3<f32>(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    return select(pow((encoded + 0.055) / 1.055, vec3<f32>(2.4)),
                  encoded / 12.92, encoded <= vec3<f32>(0.04045));
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

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return display_color(textureLoad(source, vec2<i32>(position.xy), 0).rgb);
}

@fragment
fn fragment_bloom(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return display_color(bloom_radiance(position));
}

fn bloom_radiance(position: vec4<f32>) -> vec3<f32> {
    let uv = position.xy / vec2<f32>(textureDimensions(source));
    let glow = textureSampleLevel(bloom, bloom_sampler, uv, 0.0).rgb * settings.tone.y;
    return textureLoad(source, vec2<i32>(position.xy), 0).rgb + glow;
}

@fragment
fn fragment_lut(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return lut_color(display_color(textureLoad(source, vec2<i32>(position.xy), 0).rgb));
}

@fragment
fn fragment_bloom_lut(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return lut_color(display_color(bloom_radiance(position)));
}
