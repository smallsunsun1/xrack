@group(0) @binding(9) var indirect_light: texture_2d<f32>;
@group(0) @binding(10) var indirect_guides: texture_2d<f32>;

@fragment
fn trace_indirect(@builtin(position) position: vec4<f32>) -> TraceOutput {
    let uv = position.xy / settings.indirect_buffer.xy;
    let pixel = address(uv);
    let sample = nearest_sample(pixel);
    let n = indirect_normal(pixel, sample);
    let receives_bounce = settings.indirect.z > 0.0 && any(diffuse_response(pixel, sample) > vec3<f32>(0.0));
    let receives_occlusion = settings.indirect.w > 0.0 && any(ambient_at(pixel, sample) > vec3<f32>(0.0));
    if dot(n, n) < 0.5 || (!receives_bounce && !receives_occlusion) {
        return TraceOutput(vec4<f32>(0.0), vec4<f32>(0.0));
    }
    let center_uv = (vec2<f32>(pixel) + 0.5) / settings.extents.xy;
    let origin = point(center_uv, scene_depth(pixel));
    let reference = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.z) > 0.9);
    let tangent = normalize(cross(reference, n));
    let bitangent = cross(n, tangent);
    let rotation = f32((u32(position.x) * 13u + u32(position.y) * 7u) & 15u) * 0.39269908;
    let count = u32(settings.indirect_buffer.z);
    var bounce = vec3<f32>(0.0);
    var obscured = 0.0;
    for (var index = 0u; index < count; index += 1u) {
        // Cosine-weighted hemisphere: the sampling PDF cancels the
        // receiver cosine and Lambert's 1/pi in the mean.
        let fraction = (f32(index) + 0.5) / f32(count);
        let angle = rotation + 2.3999632 * f32(index);
        let hemisphere = sqrt(fraction);
        let ray = (tangent * cos(angle) + bitangent * sin(angle)) * hemisphere
            + n * sqrt(1.0 - fraction);
        let hit = trace_ray(origin, n, ray,
            vec3<f32>(settings.indirect.xy, settings.indirect_buffer.w), true);
        if !hit.hit {
            continue;
        }
        let falloff = 1.0 - smoothstep(0.0, settings.indirect.x, hit.distance);
        obscured += falloff;
        if receives_bounce {
            let source = address(hit.uv);
            let sender = indirect_normal(source, nearest_sample(source));
            let facing = smoothstep(0.0, 0.35, dot(sender, -ray));
            bounce += textureLoad(radiance, source, 0).rgb * (facing * falloff);
        }
    }
    return TraceOutput(
        vec4<f32>(bounce, obscured) / f32(count),
        vec4<f32>(encode_screen_normal(n), log2(origin.z), 1.0),
    );
}

fn guide_weight(guide: vec4<f32>, n: vec3<f32>, depth: f32) -> f32 {
    if guide.w <= 0.0 {
        return 0.0;
    }
    let scale = max(settings.indirect.y, depth * 0.02);
    let geometry = 1.0 - smoothstep(0.0, scale, abs(depth - exp2(guide.z)));
    let normal = smoothstep(0.75, 0.95, dot(decode_screen_normal(guide.xy), n));
    return geometry * normal;
}

@fragment
fn denoise_indirect(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(position.xy);
    let guide = textureLoad(indirect_guides, pixel, 0);
    if guide.w <= 0.0 {
        return vec4<f32>(0.0);
    }
    let n = decode_screen_normal(guide.xy);
    let depth = exp2(guide.z);
    let limit = vec2<i32>(settings.indirect_buffer.xy) - 1;
    var total = vec4<f32>(0.0);
    var sum = 0.0;
    for (var y = -1; y <= 1; y += 1) {
        for (var x = -1; x <= 1; x += 1) {
            let at = clamp(pixel + vec2<i32>(x, y), vec2<i32>(0), limit);
            let tap = textureLoad(indirect_guides, at, 0);
            let weight = f32((2 - abs(x)) * (2 - abs(y))) * guide_weight(tap, n, depth);
            total += textureLoad(indirect_light, at, 0) * weight;
            sum += weight;
        }
    }
    return total / max(sum, 1e-5);
}

fn indirect_delta(position: vec2<f32>, sample: u32) -> vec3<f32> {
    let pixel = vec2<i32>(position);
    let n = indirect_normal(pixel, sample);
    if dot(n, n) < 0.5 {
        return vec3<f32>(0.0);
    }
    let response = diffuse_response(pixel, sample) * settings.indirect.z;
    let ambient = ambient_at(pixel, sample) * settings.indirect.w;
    if all(response == vec3<f32>(0.0)) && all(ambient == vec3<f32>(0.0)) {
        return vec3<f32>(0.0);
    }
    let depth = view_depth(sample_depth(pixel, sample));
    let site = position / settings.extents.xy * settings.indirect_buffer.xy - 0.5;
    let base = vec2<i32>(floor(site));
    let fraction = fract(site);
    var gathered = vec4<f32>(0.0);
    for (var index = 0; index < 4; index += 1) {
        let offset = vec2<i32>(index & 1, index >> 1);
        let at = clamp(base + offset, vec2<i32>(0), vec2<i32>(settings.indirect_buffer.xy) - 1);
        let bilinear = select(1.0 - fraction, fraction, offset != vec2<i32>(0));
        let guide = textureLoad(indirect_guides, at, 0);
        gathered += textureLoad(indirect_light, at, 0) * (bilinear.x * bilinear.y * guide_weight(guide, n, depth));
    }
    return gathered.rgb * response - ambient * clamp(gathered.a, 0.0, 1.0);
}
