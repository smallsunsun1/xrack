override DISPLAY_SIGNAL: bool = true;

struct Settings {
    // Viewport width and height, search steps, ordered dithering.
    viewport: vec4<f32>,
    // Relative and absolute contrast limits, subpixel quality, strength.
    quality: vec4<f32>,
}

@group(0) @binding(0) var color_input: texture_2d<f32>;
@group(0) @binding(1) var filtering: sampler;
@group(0) @binding(2) var<uniform> settings: Settings;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let coordinate = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(coordinate * 2.0 - 1.0, 0.0, 1.0);
}

fn color_at(pixel: vec2<i32>) -> vec4<f32> {
    let max_pixel = vec2<i32>(settings.viewport.xy) - vec2<i32>(1);
    return textureLoad(color_input, clamp(pixel, vec2<i32>(0), max_pixel), 0);
}

fn filtered(pixel: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(color_input, filtering, pixel / settings.viewport.xy, 0.0);
}

fn signal(color: vec4<f32>) -> vec2<f32> {
    let luma = dot(color.rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
    return vec2<f32>(select(sqrt(max(0.0, luma)), luma, DISPLAY_SIGNAL), color.a);
}

fn luma_at(pixel: vec2<i32>, channel: u32) -> f32 {
    return signal(color_at(pixel))[channel];
}

fn softened(position: vec2<f32>) -> vec4<f32> {
    let pixel = vec2<i32>(position);
    let original = color_at(pixel);
    let c = signal(original);
    let west = signal(color_at(pixel + vec2<i32>(-1, 0)));
    let east = signal(color_at(pixel + vec2<i32>(1, 0)));
    let north = signal(color_at(pixel + vec2<i32>(0, -1)));
    let south = signal(color_at(pixel + vec2<i32>(0, 1)));
    let high = max(c, max(max(west, east), max(north, south)));
    let low = min(c, min(min(west, east), min(north, south)));
    let ranges = high - low;
    let edge = select(0u, 1u, ranges.y > ranges.x);
    let span = ranges[edge];
    if span < max(settings.quality.y, settings.quality.x * high[edge]) {
        return original;
    }
    let m = c[edge];
    let w = west[edge];
    let e = east[edge];
    let n = north[edge];
    let s = south[edge];
    let nw = luma_at(pixel + vec2<i32>(-1, -1), edge);
    let ne = luma_at(pixel + vec2<i32>(1, -1), edge);
    let sw = luma_at(pixel + vec2<i32>(-1, 1), edge);
    let se = luma_at(pixel + vec2<i32>(1, 1), edge);
    let horizontal = 2.0 * abs(n + s - 2.0 * m) + abs(nw + sw - 2.0 * w) + abs(ne + se - 2.0 * e);
    let vertical = 2.0 * abs(w + e - 2.0 * m) + abs(nw + ne - 2.0 * n) + abs(sw + se - 2.0 * s);
    let along_x = horizontal >= vertical;
    let tangent = select(vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), along_x);
    let across = select(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0), along_x);
    let minus = select(w, n, along_x);
    let plus = select(e, s, along_x);
    let minus_gradient = abs(minus - m);
    let plus_gradient = abs(plus - m);
    let use_minus = minus_gradient >= plus_gradient;
    let direction = across * select(1.0, -1.0, use_minus);
    let midpoint = 0.5 * (m + select(plus, minus, use_minus));
    let stop = 0.25 * max(minus_gradient, plus_gradient);
    let origin = position + 0.5 * direction;

    // Search in both directions on the edge, using wider strides farther away.
    let strides = array<f32, 8>(1.0, 1.5, 2.0, 2.0, 4.0, 8.0, 8.0, 8.0);
    var distances = vec2<f32>(0.0);
    var difference = vec2<f32>(0.0);
    for (var step = 0u; step < u32(settings.viewport.z); step += 1u) {
        if abs(difference.x) < stop {
            distances.x += strides[step];
            difference.x = signal(filtered(origin - tangent * distances.x))[edge] - midpoint;
        }
        if abs(difference.y) < stop {
            distances.y += strides[step];
            difference.y = signal(filtered(origin + tangent * distances.y))[edge] - midpoint;
        }
        if all(abs(difference) >= vec2<f32>(stop)) {
            break;
        }
    }
    let near_minus = distances.x < distances.y;
    let nearest = select(difference.y, difference.x, near_minus);
    let valid_side = (nearest < 0.0) != (m < midpoint);
    let offset = select(0.0, 0.5 - min(distances.x, distances.y)
        / max(1.0, distances.x + distances.y), valid_side);
    let neighborhood = (2.0 * (w + e + n + s) + nw + ne + sw + se) / 12.0;
    let subpixel = smoothstep(0.0, 1.0, abs(neighborhood - m) / span);
    let weight = max(offset, subpixel * subpixel * settings.quality.z);
    return mix(original, filtered(position + direction * weight), settings.quality.w);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let color = softened(position.xy);
    if DISPLAY_SIGNAL {
        let encoded = dither_display_rgb(color.rgb, color.a, position.xy, settings.viewport.w > 0.5);
        return vec4<f32>(decode_srgb(encoded), color.a);
    }
    return color;
}
