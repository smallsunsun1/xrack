struct Settings {
    lens: vec4<f32>,
    // Near, far (zero means infinite), orthographic flag, strength.
    projection: vec4<f32>,
    // Travel, depth thickness, roughness cutoff, maximum steps.
    trace: vec4<f32>,
    // Opaque raster extent and reduced reflection extent.
    extents: vec4<f32>,
    // Indirect radius, hit thickness, diffuse strength and occlusion strength.
    indirect: vec4<f32>,
    // Indirect buffer extent, rays per pixel and steps per ray.
    indirect_buffer: vec4<f32>,
}

@group(0) @binding(0) var<uniform> settings: Settings;
@group(0) @binding(1) var radiance: texture_2d<f32>;
@group(0) @binding(6) var hits: texture_2d<f32>;
@group(0) @binding(8) var hit_guides: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    return vec4<f32>(p, 0.0, 1.0);
}

fn orthographic() -> bool {
    return settings.projection.z > 0.5;
}

fn view_depth(depth: f32) -> f32 {
    let near = settings.projection.x;
    let far = settings.projection.y;
    if orthographic() {
        return mix(near, far, depth);
    }
    let ratio = select(0.0, near / max(far, near), far > 0.0);
    return near / max((1.0 - depth) + depth * ratio, 1e-12);
}

fn point(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let z = view_depth(depth);
    let ndc = (uv * 2.0 - 1.0) * vec2<f32>(1.0, -1.0);
    let plane = (ndc - settings.lens.zw) / settings.lens.xy;
    return vec3<f32>(plane * select(z, 1.0, orthographic()), z);
}

fn project(p: vec3<f32>) -> vec2<f32> {
    let ndc = p.xy / select(p.z, 1.0, orthographic()) * settings.lens.xy + settings.lens.zw;
    return ndc * vec2<f32>(0.5, -0.5) + 0.5;
}

fn segment_depth(a: f32, b: f32, fraction: f32) -> f32 {
    if orthographic() {
        return mix(a, b, fraction);
    }
    return 1.0 / mix(1.0 / a, 1.0 / b, fraction);
}

fn within(uv: vec2<f32>) -> bool {
    return all(uv >= vec2<f32>(0.0)) && all(uv < vec2<f32>(1.0));
}

fn address(uv: vec2<f32>) -> vec2<i32> {
    return vec2<i32>(uv * settings.extents.xy);
}

fn error_at(a: vec3<f32>, b: vec3<f32>, uv: vec2<f32>, t: f32) -> f32 {
    let depth = scene_depth(address(uv));
    if depth >= 1.0 {
        return -1.0;
    }
    return segment_depth(a.z, b.z, t) - view_depth(depth);
}

struct ScreenHit {
    uv: vec2<f32>,
    distance: f32,
    hit: bool,
}

fn trace_ray(origin: vec3<f32>, n: vec3<f32>, ray: vec3<f32>, limits: vec3<f32>, distribute: bool) -> ScreenHit {
    let radius = limits.x;
    let thickness = limits.y;
    let step_limit = limits.z;
    let bias = clamp(thickness * 0.3, 0.01, 0.12);
    let start = origin + n * bias + ray * bias;
    let near = settings.projection.x * 1.01;
    if start.z <= near {
        return ScreenHit(vec2<f32>(0.0), 0.0, false);
    }
    var travel = radius;
    if ray.z < 0.0 {
        travel = min(travel, (start.z - near) / -ray.z);
    }
    let end = start + ray * travel;
    let uv0 = project(start);
    let uv1 = project(end);
    let pixels = abs((uv1 - uv0) * settings.extents.xy);
    let stride = max(pixels.x, pixels.y);
    // Very long rays may head towards the near plane or outside the image.
    // Keep the sampling density instead of distributing a fixed number of
    // samples over that entire, possibly off-screen segment.
    let ideal_steps = max(1.0, ceil(stride * 0.65));
    let steps = select(ideal_steps, min(ideal_steps, step_limit), distribute);
    let origin_pixel = project(origin) * settings.extents.xy;
    let count = u32(min(steps, step_limit));
    for (var index = 1u; index <= count; index += 1u) {
        let fraction = f32(index) / steps;
        let candidate = mix(uv0, uv1, fraction);
        if !within(candidate) {
            break;
        }
        if distance(candidate * settings.extents.xy, origin_pixel) < 2.0 {
            continue;
        }
        let error = error_at(start, end, candidate, fraction);
        if error < 0.0 {
            continue;
        }
        var lo = f32(index - 1u) / steps;
        var hi = fraction;
        for (var j = 0; j < 4; j += 1) {
            let mid = 0.5 * (lo + hi);
            let at = mix(uv0, uv1, mid);
            if within(at) && error_at(start, end, at, mid) >= 0.0 {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        let hit_uv = mix(uv0, uv1, hi);
        let gap = error_at(start, end, hit_uv, hi);
        if gap < 0.0 || gap > thickness {
            // We crossed the edge of a foreground object with no plausible
            // surface intersection. Looking behind it is not observable.
            break;
        }
        let perspective_fraction = (hi / end.z) / mix(1.0 / start.z, 1.0 / end.z, hi);
        return ScreenHit(hit_uv, travel * select(perspective_fraction, hi, orthographic()), true);
    }
    return ScreenHit(vec2<f32>(0.0), 0.0, false);
}

struct TraceOutput {
    @location(0) reflection: vec4<f32>,
    @location(1) guide: vec4<f32>,
}

@fragment
fn trace(@builtin(position) position: vec4<f32>) -> TraceOutput {
    let uv = position.xy / settings.extents.zw;
    let pixel = address(uv);
    let surface = surface_at(pixel, nearest_sample(pixel));
    if dot(surface.xyz, surface.xyz) < 0.5 || surface.w >= settings.trace.z {
        return TraceOutput(vec4<f32>(0.0), vec4<f32>(0.0));
    }
    let n = normalize(surface.xyz);
    let center_uv = (vec2<f32>(pixel) + 0.5) / settings.extents.xy;
    let origin = point(center_uv, scene_depth(pixel));
    let eye_ray = select(normalize(origin), vec3<f32>(0.0, 0.0, 1.0), orthographic());
    let ray = reflect(eye_ray, n);
    let result = trace_ray(origin, n, ray, settings.trace.xyw, false);
    if !result.hit {
        return TraceOutput(vec4<f32>(0.0), vec4<f32>(0.0));
    }
    let hit_uv = result.uv;
    let hit = address(hit_uv);
    var color = textureLoad(radiance, hit, 0).rgb;
    let spread = i32(ceil(surface.w * surface.w * 7.0));
    var weight = 1.0;
    if spread > 0 {
        let z = view_depth(scene_depth(hit));
        let taps = array<vec2<i32>, 4>(vec2<i32>(1, 0), vec2<i32>(-1, 0), vec2<i32>(0, 1), vec2<i32>(0, -1));
        for (var tap = 0; tap < 4; tap += 1) {
            let at = clamp(hit + taps[tap] * spread, vec2<i32>(0), vec2<i32>(settings.extents.xy) - 1);
            let raw = scene_depth(at);
            if raw < 1.0 && abs(view_depth(raw) - z) <= settings.trace.y * 2.0 {
                color += textureLoad(radiance, at, 0).rgb;
                weight += 1.0;
            }
        }
    }
    let edge = min(hit_uv, 1.0 - hit_uv);
    let edge_fade = smoothstep(0.0, 0.065, min(edge.x, edge.y));
    let roughness_fade = 1.0 - smoothstep(settings.trace.z * 0.72, settings.trace.z, surface.w);
    let range_fade = 1.0 - smoothstep(settings.trace.x * 0.75, settings.trace.x,
        result.distance);
    let confidence = edge_fade * roughness_fade * range_fade;
    return TraceOutput(
        vec4<f32>(color / weight * confidence, confidence),
        vec4<f32>(encode_screen_normal(n), surface.w, log2(origin.z)),
    );
}

fn reflection_delta(position: vec2<f32>, sample: u32) -> vec3<f32> {
    if settings.projection.w <= 0.0 {
        return vec3<f32>(0.0);
    }
    let pixel = vec2<i32>(position);
    let surface = surface_at(pixel, sample);
    if dot(surface.xyz, surface.xyz) < 0.5 || surface.w >= settings.trace.z {
        return vec3<f32>(0.0);
    }
    let center_depth = view_depth(sample_depth(pixel, sample));
    let site = position / settings.extents.xy * settings.extents.zw - 0.5;
    let base = vec2<i32>(floor(site));
    let fraction = fract(site);
    var gathered = vec4<f32>(0.0);
    for (var index = 0; index < 4; index += 1) {
        let offset = vec2<i32>(index & 1, index >> 1);
        let at = clamp(base + offset, vec2<i32>(0), vec2<i32>(settings.extents.zw) - 1);
        let hit = textureLoad(hits, at, 0);
        if hit.a <= 0.0 {
            continue;
        }
        let guide = textureLoad(hit_guides, at, 0);
        if dot(decode_screen_normal(guide.xy), surface.xyz) < 0.85 || abs(guide.z - surface.w) > 0.2 {
            continue;
        }
        let distance_scale = max(settings.trace.y, center_depth * 0.015);
        let bilateral = 1.0 - smoothstep(0.0, distance_scale, abs(center_depth - exp2(guide.w)));
        let xy = select(1.0 - fraction, fraction, offset != vec2<i32>(0));
        gathered += hit * (xy.x * xy.y * bilateral);
    }
    let response = response_at(pixel, sample);
    let fallback = probe_at(pixel, sample);
    // Replace only the selected specular lobe. A miss leaves the probe and
    // the rest of the original illumination unchanged.
    return settings.projection.w * (gathered.rgb * response - gathered.a * fallback);
}

@fragment
fn composite(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let opaque = textureLoad(radiance, vec2<i32>(position.xy), 0);
    let delta = reflection_delta(position.xy, 0u) + indirect_delta(position.xy, 0u);
    return vec4<f32>(clamp(opaque.rgb + delta, vec3<f32>(0.0), vec3<f32>(65000.0)), opaque.a);
}

@fragment
fn composite_samples(@builtin(position) position: vec4<f32>, @builtin(sample_index) sample: u32) -> @location(0) vec4<f32> {
    let delta = reflection_delta(position.xy, sample) + indirect_delta(position.xy, sample);
    let limit = sample_headroom(vec2<i32>(position.xy), sample);
    return vec4<f32>(min(delta, vec3<f32>(limit)), 0.0);
}
