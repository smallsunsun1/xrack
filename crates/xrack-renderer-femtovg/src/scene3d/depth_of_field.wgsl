struct Settings {
    // Near, far (zero for infinite perspective), orthographic flag, alpha flag.
    projection: vec4<f32>,
    // Focus distance, half of the in-focus range, physical blur radii (x, y).
    focus: vec4<f32>,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var<uniform> settings: Settings;
@group(0) @binding(2) var depth: texture_depth_2d;
@group(0) @binding(3) var depth_msaa: texture_depth_multisampled_2d;
@group(0) @binding(4) var reduced: texture_2d<f32>;
@group(0) @binding(5) var blurred: texture_2d<f32>;
@group(0) @binding(6) var foreground: texture_2d<f32>;
@group(0) @binding(7) var foreground_depth: texture_2d<f32>;
@group(0) @binding(8) var reduced_sampler: sampler;
@group(0) @binding(9) var reduced_coverage: texture_2d<f32>;

const SAMPLE_COUNT = 24u;
const DISK = array<vec2<f32>, 24>(
    vec2<f32>(0.1443, 0.0000), vec2<f32>(-0.1843, 0.1689),
    vec2<f32>(0.0282, -0.3215), vec2<f32>(0.2324, 0.3031),
    vec2<f32>(-0.4264, -0.0754), vec2<f32>(0.4039, -0.2569),
    vec2<f32>(-0.1351, 0.5026), vec2<f32>(-0.2577, -0.4961),
    vec2<f32>(0.5590, 0.2041), vec2<f32>(-0.5816, 0.2401),
    vec2<f32>(0.2803, -0.5991), vec2<f32>(0.2072, 0.6605),
    vec2<f32>(-0.6244, -0.3619), vec2<f32>(0.7325, -0.1610),
    vec2<f32>(-0.4470, 0.6359), vec2<f32>(-0.1033, -0.7970),
    vec2<f32>(0.6340, 0.5343), vec2<f32>(-0.8532, 0.0353),
    vec2<f32>(0.6223, -0.6193), vec2<f32>(-0.0416, 0.9004),
    vec2<f32>(-0.5922, -0.7096), vec2<f32>(0.9380, 0.1262),
    vec2<f32>(-0.7948, 0.5530), vec2<f32>(0.2172, -0.9654)
);

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn linear_depth(clip: f32) -> f32 {
    let near = settings.projection.x;
    let far = settings.projection.y;
    if settings.projection.z > 0.5 {
        return mix(near, far, clip);
    }
    var ratio = 0.0;
    if far > 0.0 {
        ratio = near / far;
    }
    return near / max((1.0 - clip) + clip * ratio, 1e-12);
}

// Keep far-world values representable in the half-float intermediate.
fn encode_depth(depth: f32) -> f32 {
    let far = settings.projection.y;
    if far > 0.0 {
        return depth / far;
    }
    return log2(1.0 + depth / settings.projection.x);
}

fn decode_depth(encoded: f32) -> f32 {
    let far = settings.projection.y;
    if far > 0.0 {
        return encoded * far;
    }
    return settings.projection.x * (exp2(encoded) - 1.0);
}

fn nearest_sample(pixel: vec2<i32>) -> f32 {
    var clip = 1.0;
    for (var sample = 0u; sample < textureNumSamples(depth_msaa); sample += 1u) {
        clip = min(clip, textureLoad(depth_msaa, pixel, sample));
    }
    return clip;
}

fn circle_of_confusion(view_depth: f32) -> f32 {
    let delta = view_depth - settings.focus.x;
    let transition = max(settings.focus.x * 0.30, max(settings.focus.y, 0.025));
    let radius = smoothstep(0.0, transition, abs(delta) - settings.focus.y);
    return sign(delta) * radius;
}

fn depth_tolerance(view_depth: f32) -> f32 {
    return max(view_depth * 0.025, 0.01);
}

struct ReducedLayers {
    @location(0) color_depth: vec4<f32>,
    @location(1) coverage: f32,
}

fn prefilter(pixels: array<vec2<i32>, 4>, clips: vec4<f32>) -> ReducedLayers {
    let nearest = linear_depth(min(min(clips.x, clips.y), min(clips.z, clips.w)));
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var i = 0u; i < 4u; i += 1u) {
        let depth = linear_depth(clips[i]);
        let w = max(1.0 - (depth - nearest) / depth_tolerance(nearest), 0.0);
        sum += textureLoad(source, pixels[i], 0) * w;
        weight += w;
    }
    let averaged = sum / max(weight, 1.0);
    return ReducedLayers(vec4<f32>(averaged.rgb, encode_depth(nearest)), averaged.a);
}

fn quad(point: vec4<f32>) -> array<vec2<i32>, 4> {
    let pixel = vec2<i32>(point.xy) * 2;
    let limit = vec2<i32>(textureDimensions(source)) - 1;
    return array<vec2<i32>, 4>(
        min(pixel, limit), min(pixel + vec2<i32>(1, 0), limit),
        min(pixel + vec2<i32>(0, 1), limit), min(pixel + vec2<i32>(1, 1), limit)
    );
}

fn reduce_sample(point: vec4<f32>) -> ReducedLayers {
    let pixels = quad(point);
    return prefilter(pixels, vec4<f32>(
        textureLoad(depth, pixels[0], 0), textureLoad(depth, pixels[1], 0),
        textureLoad(depth, pixels[2], 0), textureLoad(depth, pixels[3], 0)
    ));
}

fn reduce_sample_msaa(point: vec4<f32>) -> ReducedLayers {
    let pixels = quad(point);
    return prefilter(pixels, vec4<f32>(
        nearest_sample(pixels[0]), nearest_sample(pixels[1]),
        nearest_sample(pixels[2]), nearest_sample(pixels[3])
    ));
}

@fragment
fn reduce(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    return reduce_sample(point).color_depth;
}

@fragment
fn reduce_msaa(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    return reduce_sample_msaa(point).color_depth;
}

@fragment
fn reduce_alpha(@builtin(position) point: vec4<f32>) -> ReducedLayers {
    return reduce_sample(point);
}

@fragment
fn reduce_alpha_msaa(@builtin(position) point: vec4<f32>) -> ReducedLayers {
    return reduce_sample_msaa(point);
}

struct Components {
    @location(0) base: vec4<f32>,
    @location(1) near_color: vec4<f32>,
    @location(2) near_depth: f32,
}

fn sample_color(at: vec2<f32>, limit: vec2<i32>, nearest: vec4<f32>) -> vec4<f32> {
    let uv = (at + 0.5) / vec2<f32>(limit + 1);
    let filtered = textureSampleLevel(reduced, reduced_sampler, uv, 0.0);
    let depth = decode_depth(nearest.a);
    // Interpolate subpixel colors and coverage only within the same depth layer.
    let matches_depth = abs(decode_depth(filtered.a) - depth) < depth_tolerance(depth);
    var alpha = 1.0;
    if settings.projection.w > 0.5 {
        if matches_depth {
            alpha = textureSampleLevel(reduced_coverage, reduced_sampler, uv, 0.0).r;
        } else {
            alpha = textureLoad(reduced_coverage, vec2<i32>(round(at)), 0).r;
        }
    }
    return vec4<f32>(select(nearest.rgb, filtered.rgb, matches_depth), alpha);
}

@fragment
fn gather(@builtin(position) point: vec4<f32>) -> Components {
    let pixel = vec2<i32>(point.xy);
    let limit = vec2<i32>(textureDimensions(reduced)) - 1;
    let center = textureLoad(reduced, pixel, 0);
    let center_depth = decode_depth(center.a);
    let center_coc = circle_of_confusion(center_depth);
    let half_radius = settings.focus.zw * 0.5;
    let max_radius = max(half_radius.x, half_radius.y);
    let base_radius = abs(center_coc) * half_radius;
    let bias = depth_tolerance(center_depth);
    var alpha = 1.0;
    if settings.projection.w > 0.5 {
        alpha = textureLoad(reduced_coverage, pixel, 0).r;
    }
    var base = vec4<f32>(center.rgb, alpha);
    var base_count = 1.0;
    var near_color = vec4<f32>(0.0);
    var nearest = linear_depth(1.0);
    for (var i = 0u; i < SAMPLE_COUNT; i += 1u) {
        let direction = DISK[i];
        if max(base_radius.x, base_radius.y) >= 0.5 {
            let at = clamp(vec2<f32>(pixel) + direction * base_radius, vec2<f32>(0.0), vec2<f32>(limit));
            let sample = textureLoad(reduced, vec2<i32>(round(at)), 0);
            let z = decode_depth(sample.a);
            if z >= center_depth - bias || (center_coc > 0.0 && circle_of_confusion(z) > 0.0) {
                base += sample_color(at, limit, sample);
                base_count += 1.0;
            }
        }
        let offset = direction * half_radius;
        let at = clamp(vec2<f32>(pixel) + offset, vec2<f32>(0.0), vec2<f32>(limit));
        let near = textureLoad(reduced, vec2<i32>(round(at)), 0);
        let z = decode_depth(near.a);
        let radius = -circle_of_confusion(z) * max_radius;
        if radius > 0.5 && z <= center_depth + bias {
            let weight = clamp((radius - length(direction * max_radius)) / 1.5, 0.0, 1.0);
            if weight > 0.0 {
                near_color += sample_color(at, limit, near) * weight;
                nearest = min(nearest, z);
            }
        }
    }
    let base_color = base / base_count;
    return Components(vec4<f32>(base_color.rgb, select(center.a, base_color.a, settings.projection.w > 0.5)),
                      near_color / f32(SAMPLE_COUNT), nearest);
}

fn composite_pixel(pixel: vec2<i32>, clip: f32) -> vec4<f32> {
    let sharp = textureLoad(source, pixel, 0);
    let composite_alpha = settings.projection.w > 0.5;
    let linear = linear_depth(clip);
    let coc = abs(circle_of_confusion(linear));
    let size = vec2<i32>(textureDimensions(blurred));
    let at = (vec2<f32>(pixel) + 0.5) * vec2<f32>(size) / vec2<f32>(textureDimensions(source)) - 0.5;
    let tile = vec2<i32>(floor(at));
    let fraction = fract(at);
    let tolerance = depth_tolerance(linear);
    var base = vec4<f32>(0.0);
    var base_weight = 0.0;
    var overlay = vec4<f32>(0.0);
    for (var i = 0u; i < 4u; i += 1u) {
        let offset = vec2<i32>(i32(i & 1u), i32(i >> 1u));
        let tap = clamp(tile + offset, vec2<i32>(0), size - 1);
        let bilinear = select(1.0 - fraction, fraction, offset != vec2<i32>(0));
        let weight = bilinear.x * bilinear.y;
        if coc > 0.0 {
            let sample = textureLoad(blurred, tap, 0);
            var depth = sample.a;
            var alpha = 1.0;
            if composite_alpha {
                depth = textureLoad(reduced, tap, 0).a;
                alpha = sample.a;
            }
            let matching = max(1.0 - abs(decode_depth(depth) - linear) / tolerance, 0.0) * weight;
            base += vec4<f32>(sample.rgb, alpha) * matching;
            base_weight += matching;
        }
        let near = textureLoad(foreground, tap, 0);
        if near.a > 0.0 && textureLoad(foreground_depth, tap, 0).r <= linear + tolerance {
            overlay += near * weight;
        }
    }
    let mixing = coc * min(1.0, base_weight * 3.0);
    let color = mix(sharp, base / max(base_weight, 1e-6), mixing);
    let premultiplied = color * (1.0 - overlay.a) + overlay;
    return vec4<f32>(premultiplied.rgb, select(1.0, premultiplied.a, composite_alpha));
}

@fragment
fn composite(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(point.xy);
    return composite_pixel(pixel, textureLoad(depth, pixel, 0));
}

@fragment
fn composite_msaa(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(point.xy);
    return composite_pixel(pixel, nearest_sample(pixel));
}
