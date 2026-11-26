// The volume uniforms follow the camera and material fog settings.
struct FogVolume {
    center_density: vec4<f32>,
    half_size_shape: vec4<f32>,
    // Linear color and noise modulation amount.
    color: vec4<f32>,
    // World Y at clear and dense endpoints, inverse span, enabled.
    height: vec4<f32>,
    local_x: vec4<f32>,
    local_y: vec4<f32>,
    local_z: vec4<f32>,
    // Periodic noise offset and reciprocal world period.
    noise: vec4<f32>,
}

struct FogRay {
    origin: vec3<f32>,
    direction: vec3<f32>,
}

struct FogEvent {
    distance: f32,
    density: f32,
    source: vec3<f32>,
}

struct FogMedium {
    radiance: vec3<f32>,
    transmission: f32,
}

fn box_fog_interval(volume: FogVolume, ray: FogRay, interval: vec2<f32>) -> vec2<f32> {
    let half_size = volume.half_size_shape.xyz;
    let origin = ray.origin;
    var enter = interval.x;
    var leave = interval.y;
    for (var axis = 0u; axis < 3u; axis += 1u) {
        let direction = ray.direction[axis];
        if abs(direction) < 1e-10 {
            if abs(origin[axis]) > half_size[axis] {
                return vec2<f32>(0.0);
            }
        } else {
            let intersections = (vec2<f32>(-half_size[axis], half_size[axis]) - origin[axis]) / direction;
            enter = max(enter, min(intersections.x, intersections.y));
            leave = min(leave, max(intersections.x, intersections.y));
        }
    }
    return vec2<f32>(enter, leave);
}

fn sphere_fog_interval(volume: FogVolume, ray: FogRay, interval: vec2<f32>) -> vec2<f32> {
    let half_size = volume.half_size_shape.xyz;
    let origin = ray.origin / half_size;
    let direction = ray.direction / half_size;
    let scale = max(abs(direction.x), max(abs(direction.y), abs(direction.z)));
    if !(scale > 0.0) {
        return vec2<f32>(0.0);
    }
    let scaled = direction / scale;
    let speed = length(scaled);
    let axis = scaled / speed;
    let distance_scale = (1.0 / speed) / scale;
    let closest = -dot(origin, axis);
    let offset = origin + closest * axis;
    let radius_squared = 1.0 - dot(offset, offset);
    if !(radius_squared > 0.0) {
        return vec2<f32>(0.0);
    }
    let radius = sqrt(radius_squared);
    return vec2<f32>(
        max(interval.x, (closest - radius) * distance_scale),
        min(interval.y, (closest + radius) * distance_scale),
    );
}

fn fog_interval(volume: FogVolume, ray: FogRay, interval: vec2<f32>) -> vec2<f32> {
    var local = FogRay(ray.origin - volume.center_density.xyz, ray.direction);
    if volume.local_x.w > 0.5 {
        let x = volume.local_x.xyz;
        let y = volume.local_y.xyz;
        let z = volume.local_z.xyz;
        local.origin = vec3<f32>(dot(x, local.origin), dot(y, local.origin), dot(z, local.origin));
        local.direction = vec3<f32>(dot(x, local.direction), dot(y, local.direction), dot(z, local.direction));
    }
    if volume.half_size_shape.w > 0.5 {
        return sphere_fog_interval(volume, local, interval);
    }
    return box_fog_interval(volume, local, interval);
}

fn fog_incident_light(ray: FogRay, distance: f32) -> vec3<f32> {
    let world = ray.origin + ray.direction * distance;
    let g = globals.volume_lighting.y;
    var radiance = vec3<f32>(0.0);
    for (var index = 0u; index < u32(lighting.settings.x); index += 1u) {
        let light = lighting.lights[index];
        if light.scope != 0u {
            continue;
        }
        let incident = incident_light(light, world);
        if all(incident.radiance <= vec3<f32>(0.0)) {
            continue;
        }
        // Both the incoming and outgoing propagation directions are the negatives
        // of the directions toward the lamp and from the camera, respectively.
        let cosine = clamp(dot(ray.direction, incident.axis), -1.0, 1.0);
        let factor = 1.0 + g * g - 2.0 * g * cosine;
        let phase = (1.0 - g * g) / (12.566371 * factor * sqrt(factor));
        radiance += incident.radiance * (phase * world_shadow_visibility(light, world));
    }
    return radiance;
}

struct FogIntegration {
    medium: FogMedium,
    sample_index: u32,
}

fn fog_sample_mass(optical_depth: f32) -> f32 {
    if FOG_SCATTERING {
        return (1.0 - exp(-optical_depth)) / globals.volume_lighting.z;
    }
    return 0.0;
}

fn integrate_fog_segment(
    ray: FogRay,
    start: f32,
    length: f32,
    density: f32,
    color: vec3<f32>,
    sample_mass: f32,
    state: ptr<function, FogIntegration>,
) {
    let span_transmission = exp(-density * length);
    let incoming = (*state).medium.transmission;
    let outgoing = incoming * span_transmission;
    (*state).medium.radiance += incoming * (1.0 - span_transmission) * color;
    if FOG_SCATTERING && sample_mass > 0.0 {
        // Stratify samples in cumulative removed transmission. Each one has
        // equal weight even if its region is much denser or shorter than another.
        let max_samples = u32(globals.volume_lighting.z);
        let energy = sample_mass * globals.volume_lighting.x;
        let removed = 1.0 - outgoing;
        while (*state).sample_index < max_samples {
            let sample_loss = (f32((*state).sample_index) + 0.5) * sample_mass;
            if sample_loss >= removed {
                break;
            }
            let local_transmission = max((1.0 - sample_loss) / incoming, 1e-7);
            let distance = start + clamp(-log(local_transmission) / density, 0.0, length);
            (*state).medium.radiance += fog_incident_light(ray, distance) * color * energy;
            (*state).sample_index += 1u;
        }
    }
    (*state).medium.transmission = outgoing;
}

fn clip_fog_interval(plane: vec4<f32>, ray: FogRay, interval: vec2<f32>) -> vec2<f32> {
    let value = dot(plane, vec4<f32>(ray.origin, 1.0));
    let rate = dot(plane.xyz, ray.direction);
    if rate > 0.0 {
        return vec2<f32>(max(interval.x, -value / rate), interval.y);
    }
    if rate < 0.0 {
        return vec2<f32>(interval.x, min(interval.y, -value / rate));
    }
    return select(vec2<f32>(0.0), interval, value >= 0.0);
}

fn fog_to_distance(ray: FogRay, limit: f32) -> FogMedium {
    let count = u32(globals.volume_clip_count.z);
    if count == 0u {
        return FogMedium(vec3<f32>(0.0), 1.0);
    }
    var interval: vec2<f32>;
    if globals.oblique_depth > 0.5 {
        interval = clip_fog_interval(globals.clip2, ray, vec2<f32>(0.0, limit));
        interval = clip_fog_interval(globals.clip3 - globals.clip2, ray, interval);
    } else {
        let forward = dot(ray.direction, globals.view_depth.xyz);
        interval = vec2<f32>(globals.volume_clip_count.x / forward,
            min(limit, globals.volume_clip_count.y / forward));
    }
    if interval.y <= interval.x {
        return FogMedium(vec3<f32>(0.0), 1.0);
    }
    if FOG_DENSITY_NOISE {
        return integrate_noisy_fog(ray, interval, count);
    }
    if FOG_HEIGHT_PROFILE {
        return integrate_fog_profiles(ray, interval, count);
    }
    if count == 1u {
        let volume = globals.volumes[0u];
        let hit = fog_interval(volume, ray, interval);
        let span = max(0.0, hit.y - hit.x);
        let density = volume.center_density.w;
        if FOG_SCATTERING {
            if span == 0.0 {
                return FogMedium(vec3<f32>(0.0), 1.0);
            }
            var state = FogIntegration(FogMedium(vec3<f32>(0.0), 1.0), 0u);
            integrate_fog_segment(ray, hit.x, span, density, volume.color.rgb,
                                  fog_sample_mass(density * span), &state);
            return state.medium;
        }
        let transmission = exp(-density * span);
        return FogMedium(volume.color.rgb * (1.0 - transmission), transmission);
    }
    var events: array<FogEvent, 16>;
    var used = 0u;
    var optical_depth = 0.0;
    for (var index = 0u; index < count; index += 1u) {
        let volume = globals.volumes[index];
        let hit = fog_interval(volume, ray, interval);
        if hit.y > hit.x {
            let density = volume.center_density.w;
            if FOG_SCATTERING {
                optical_depth += density * (hit.y - hit.x);
            }
            let source = volume.color.rgb * density;
            events[used] = FogEvent(hit.x, density, source);
            events[used + 1u] = FogEvent(hit.y, -density, -source);
            used += 2u;
        }
    }
    // Sort the entry and exit points, then integrate each constant-density interval.
    for (var i = 1u; i < used; i += 1u) {
        let event = events[i];
        var slot = i;
        while slot > 0u {
            if events[slot - 1u].distance <= event.distance {
                break;
            }
            events[slot] = events[slot - 1u];
            slot -= 1u;
        }
        events[slot] = event;
    }
    var state = FogIntegration(FogMedium(vec3<f32>(0.0), 1.0), 0u);
    let sample_mass = fog_sample_mass(optical_depth);
    var source = vec3<f32>(0.0);
    var density = 0.0;
    var overlaps = 0i;
    var previous = interval.x;
    for (var i = 0u; i < used; i += 1u) {
        let event = events[i];
        let segment_length = event.distance - previous;
        if overlaps > 0 && density > 0.0 && segment_length > 0.0 {
            integrate_fog_segment(ray, previous, segment_length, density,
                                  max(source / density, vec3<f32>(0.0)), sample_mass, &state);
        }
        overlaps += select(-1, 1, event.density > 0.0);
        density += event.density;
        source += event.source;
        if overlaps == 0 {
            density = 0.0;
            source = vec3<f32>(0.0);
        }
        previous = event.distance;
    }
    return state.medium;
}

fn fog_to_surface(world: vec3<f32>) -> FogMedium {
    if globals.oblique_depth > 1.5 {
        let depth = dot(globals.view_depth, vec4<f32>(world, 1.0));
        let forward = custom_forward(world);
        let distance = depth / max(dot(forward, globals.view_depth.xyz), 1e-8);
        return fog_to_distance(FogRay(world - distance * forward, forward), distance);
    }
    if globals.eye.w > 0.5 {
        let depth = dot(globals.view_depth, vec4<f32>(world, 1.0));
        let forward = globals.view_depth.xyz;
        return fog_to_distance(FogRay(world - depth * forward, forward), depth);
    }
    let offset = world - globals.eye.xyz;
    let distance = length(offset);
    return fog_to_distance(FogRay(globals.eye.xyz, offset / distance), distance);
}
