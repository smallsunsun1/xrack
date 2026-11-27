@group(0) @binding(XRACK_TRAIL_BINDING) var<storage, read_write> trail_points: array<TrailPoint>;

fn sample_trail_path(birth: ParticleBirth, age: f32) -> Displaced {
    return XRACK_TRAIL_SAMPLE;
}

fn trail_path_distance(start: vec3<f32>, end: vec3<f32>) -> f32 {
    return min(length(end - start), 1e20);
}

fn trail_is_linear(birth: ParticleBirth) -> bool {
    return (!XRACK_TRAIL_AFFECTED || birth.particle.emitter.playback.w == 0u)
        && all(birth.particle.emitter.gravity.xyz == vec3<f32>(0.0));
}

fn trail_birth_distance(birth: ParticleBirth, age: f32, position: vec3<f32>) -> f32 {
    if age <= 0.0 {
        return 0.0;
    }
    if trail_is_linear(birth) {
        return min(length(birth.velocity) * age, 1e20);
    }
    // Estimate the phase over the history preceding the rendered polyline.
    const STEPS: u32 = 24u;
    var previous = sample_trail_path(birth, 0.0).source.center;
    var distance = 0.0;
    for (var step = 1u; step < STEPS; step++) {
        let center = sample_trail_path(birth, age * (f32(step) / f32(STEPS))).source.center;
        distance += trail_path_distance(previous, center);
        previous = center;
    }
    return distance + trail_path_distance(previous, position);
}

struct SampledTrail {
    points: array<TrailPoint, 17>,
    last: u32,
}

fn trail_point_at(birth: ParticleBirth, age: f32, end_opacity: f32) -> TrailPoint {
    let sample = sample_trail_path(birth, age);
    let emitter = sample.source.particle.emitter;
    let phase_age = sample.source.age;
    let fraction = clamp(phase_age / emitter.spread_lifetime.w, 0.0, 1.0);
    let fade = particle_fade(emitter, phase_age);
    let radius = mix(emitter.position_radius.w, emitter.velocity_end_radius.w, fraction) * fade.y;
    let alpha = select(0.0, fade.x * end_opacity, sample.visible);
    return TrailPoint(vec4<f32>(sample.source.center, age), vec4<f32>(radius, alpha, 0.0, 0.0));
}

fn timed_trail(birth: ParticleBirth, interval: TrailInterval, span: f32, segments: u32) -> SampledTrail {
    var path: SampledTrail;
    path.last = segments;
    var distance = 0.0;
    for (var i = 0u; i <= segments; i++) {
        let age = interval.head - span * (f32(i) / f32(segments));
        var point = trail_point_at(birth, age, interval.end_opacity);
        if i > 0u {
            distance += trail_path_distance(path.points[i - 1u].center_age.xyz, point.center_age.xyz);
        }
        point.style.w = distance;
        path.points[i] = point;
    }
    return path;
}

fn distance_trail(birth: ParticleBirth, interval: TrailInterval, span: f32, segments: u32,
                  spacing: f32, limit: f32) -> SampledTrail {
    var path: SampledTrail;
    path.points[0] = trail_point_at(birth, interval.head, interval.end_opacity);
    if span <= 0.0 || limit <= 0.0 {
        return path;
    }
    if trail_is_linear(birth) {
        let speed = min(length(birth.velocity), 1e20);
        if speed <= 0.0 {
            return path;
        }
        let visible = min(span * speed, limit);
        for (var i = 1u; i <= segments; i++) {
            let distance = min(spacing * f32(i), visible);
            let age = interval.head - min(distance / speed, span);
            var point = trail_point_at(birth, age, interval.end_opacity);
            point.style.w = distance;
            path.points[i] = point;
            path.last = i;
            if distance >= visible {
                break;
            }
        }
        return path;
    }

    // Stream a denser polyline through the distance thresholds. This keeps
    // one output array per invocation and stops when the length budget is full.
    let steps = clamp(4u * segments, 16u, 64u);
    var previous = path.points[0];
    for (var sample = 1u; sample <= steps; sample++) {
        let age = interval.head - span * (f32(sample) / f32(steps));
        var current = trail_point_at(birth, age, interval.end_opacity);
        let edge = trail_path_distance(previous.center_age.xyz, current.center_age.xyz);
        current.style.w = previous.style.w + edge;
        while path.last < segments {
            let next_index = path.last + 1u;
            let threshold = min(spacing * f32(next_index), limit);
            if threshold > current.style.w || current.style.w <= previous.style.w {
                break;
            }
            let t = clamp((threshold - previous.style.w) / (current.style.w - previous.style.w), 0.0, 1.0);
            path.points[next_index] = TrailPoint(mix(previous.center_age, current.center_age, t),
                vec4<f32>(mix(previous.style.xy, current.style.xy, t), 0.0, threshold));
            path.last = next_index;
        }
        if path.last == segments {
            return path;
        }
        previous = current;
    }
    // Keep the shorter terminal segment when the history ends between marks.
    if previous.style.w > path.points[path.last].style.w {
        path.last += 1u;
        path.points[path.last] = previous;
    }
    return path;
}

@compute @workgroup_size(64)
fn compute_trail(@builtin(global_invocation_id) invocation: vec3<u32>) {
    let segments = trails.segment_count.x;
    let point_count = segments + 1u;
    let stride = point_count + XRACK_TRAIL_HEADER_ROWS;
    let index = invocation.x;
    if index >= arrayLength(&trail_points) / stride {
        return;
    }
    let birth = XRACK_TRAIL_BIRTH(index);
    let first = index * stride;
    if XRACK_TRAIL_HEADER_ROWS != 0u {
        trail_points[first] = TrailPoint(vec4<f32>(birth.velocity, 0.0), vec4<f32>(0.0));
    }
    let particle = birth.particle;
    let settings = trails.settings[particle.slot];
    let interval = trail_interval(0u, particle);
    let head = interval.head;
    let span = min(head, settings.shape.x);
    let mode = u32(settings.mapping.z);

    let distance_sampled = settings.mapping.x >= 0.0 || settings.sampling.x > 0.0;
    var path: SampledTrail;
    if distance_sampled {
        var spacing = settings.sampling.x;
        var limit = spacing * f32(segments);
        if settings.mapping.x >= 0.0 {
            let variation = signed_random(particle.seed ^ 0x0d695733u) * settings.mapping.y;
            limit = max(settings.mapping.x + variation, 0.0);
            spacing = limit / f32(segments);
        }
        path = distance_trail(birth, interval, span, segments, spacing, limit);
    } else {
        path = timed_trail(birth, interval, span, segments);
    }

    let tail = path.points[path.last];
    let visible_length = tail.style.w;
    var head_distance = 0.0;
    if mode == 3u && span > 0.0 {
        if distance_sampled {
            head_distance = trail_birth_distance(birth, head, path.points[0].center_age.xyz);
        } else {
            head_distance = trail_birth_distance(birth, head - span, tail.center_age.xyz) + visible_length;
        }
    }
    for (var i = 0u; i < point_count; i++) {
        let point_index = min(i, path.last);
        var point = path.points[point_index];
        let distance = point.style.w;
        var time_fraction = f32(i) / f32(segments);
        if distance_sampled {
            time_fraction = clamp((head - point.center_age.w) / max(head - tail.center_age.w, 1e-20), 0.0, 1.0);
        }
        var texcoord = time_fraction;
        switch mode {
            case 1u: { texcoord = distance / max(visible_length, 1e-20); }
            case 2u: { texcoord = distance; }
            case 3u: { texcoord = max(head_distance - distance, 0.0); }
            default: {}
        }
        point.style.x *= mix(1.0, settings.shape.y, time_fraction)
            * trail_segment_factor(settings.sampling.y, point_index);
        point.style.y *= (1.0 - settings.shape.z * time_fraction)
            * trail_segment_factor(settings.sampling.z, point_index);
        point.style.z = texcoord * settings.mapping.w;
        trail_points[first + XRACK_TRAIL_HEADER_ROWS + i] = point;
    }
}
