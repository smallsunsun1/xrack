fn sort_fog_boundaries(boundaries: ptr<function, array<f32, 32>>, used: u32) {
    for (var i = 1u; i < used; i += 1u) {
        let value = (*boundaries)[i];
        var slot = i;
        while slot > 0u {
            if (*boundaries)[slot - 1u] <= value {
                break;
            }
            (*boundaries)[slot] = (*boundaries)[slot - 1u];
            slot -= 1u;
        }
        (*boundaries)[slot] = value;
    }
}

// Density is affine between the world-Y ramp planes and constant outside them.
fn profile_density(volume: FogVolume, ray: FogRay, distance: f32) -> f32 {
    if volume.height.w == 0.0 {
        return volume.center_density.w;
    }
    let y = fma(ray.direction.y, distance, ray.origin.y);
    let factor = clamp((y - volume.height.x) * volume.height.z, 0.0, 1.0);
    return volume.center_density.w * factor;
}

fn profile_boundaries(volume: FogVolume, ray: FogRay, hit: vec2<f32>) -> vec4<f32> {
    if volume.height.w == 0.0 || ray.direction.y == 0.0 {
        return vec4<f32>(hit.x, hit.x, hit.y, hit.y);
    }
    let t = (volume.height.xy - vec2<f32>(ray.origin.y)) / ray.direction.y;
    let a = clamp(min(t.x, t.y), hit.x, hit.y);
    let b = clamp(max(t.x, t.y), hit.x, hit.y);
    return vec4<f32>(hit.x, a, b, hit.y);
}

fn profile_optical_depth(volume: FogVolume, ray: FogRay, cuts: vec4<f32>) -> f32 {
    var depth = 0.0;
    for (var index = 0u; index < 3u; index += 1u) {
        let a = cuts[index];
        let b = cuts[index + 1u];
        if b > a {
            depth += 0.5 * (profile_density(volume, ray, a) + profile_density(volume, ray, b)) * (b - a);
        }
    }
    return depth;
}

// Invert the quadratic integrated density, expressed as a fraction of a span.
fn profile_fraction(density: vec2<f32>, span: f32, transmission: f32) -> f32 {
    let depth = -log(max(transmission, 1e-24)) / span;
    let gradient = density.y - density.x;
    let root = sqrt(max(0.0, density.x * density.x + 2.0 * gradient * depth));
    let denominator = density.x + root;
    return clamp(2.0 * depth / max(denominator, 1e-20), 0.0, 1.0);
}

fn profile_tint(start: vec4<f32>, end: vec4<f32>, fraction: f32) -> vec3<f32> {
    let mixed = mix(start, end, fraction);
    return mixed.rgb / max(mixed.a, 1e-20);
}

fn integrate_profile_span(
    ray: FogRay,
    interval: vec2<f32>,
    before: vec4<f32>,
    after: vec4<f32>,
    colors: u32,
    sample_mass: f32,
    state: ptr<function, FogIntegration>,
) {
    let span = interval.y - interval.x;
    let density = vec2<f32>(before.a, after.a);
    let transmission = exp(-0.5 * (density.x + density.y) * span);
    let incoming = (*state).medium.transmission;
    let outgoing = incoming * transmission;
    let weight = incoming * (1.0 - transmission);
    if weight > 0.0 {
        if colors == 1u || all(before == after) {
            let combined = before + after;
            (*state).medium.radiance += weight * combined.rgb / combined.a;
        } else {
            // Two-point quadrature in opacity space handles differently colored
            // profiles while keeping their total extinction exact.
            let first = profile_fraction(density, span, mix(1.0, transmission, 0.211325));
            let second = profile_fraction(density, span, mix(1.0, transmission, 0.788675));
            (*state).medium.radiance += weight * 0.5
                * (profile_tint(before, after, first) + profile_tint(before, after, second));
        }
    }
    if FOG_SCATTERING && sample_mass > 0.0 {
        let max_samples = u32(globals.volume_lighting.z);
        let energy = sample_mass * globals.volume_lighting.x;
        let removed = 1.0 - outgoing;
        while (*state).sample_index < max_samples {
            let sample_loss = (f32((*state).sample_index) + 0.5) * sample_mass;
            if sample_loss >= removed {
                break;
            }
            let relative = profile_fraction(density, span, (1.0 - sample_loss) / incoming);
            let distance = interval.x + relative * span;
            (*state).medium.radiance += fog_incident_light(ray, distance)
                * profile_tint(before, after, relative) * energy;
            (*state).sample_index += 1u;
        }
    }
    (*state).medium.transmission = outgoing;
}

fn integrate_fog_profiles(ray: FogRay, clip: vec2<f32>, count: u32) -> FogMedium {
    if !FOG_SCATTERING && count == 1u {
        let volume = globals.volumes[0];
        let hit = fog_interval(volume, ray, clip);
        if hit.y <= hit.x {
            return FogMedium(vec3<f32>(0.0), 1.0);
        }
        let optical_depth = profile_optical_depth(volume, ray, profile_boundaries(volume, ray, hit));
        let transmission = exp(-optical_depth);
        return FogMedium(volume.color.rgb * (1.0 - transmission), transmission);
    }

    var hits: array<vec2<f32>, 8>;
    var boundaries: array<f32, 32>;
    var used = 0u;
    var optical_depth = 0.0;
    for (var index = 0u; index < count; index += 1u) {
        let volume = globals.volumes[index];
        let hit = fog_interval(volume, ray, clip);
        hits[index] = hit;
        if hit.y > hit.x {
            let cuts = profile_boundaries(volume, ray, hit);
            for (var cut = 0u; cut < 4u; cut += 1u) {
                if cut == 0u || cuts[cut] != cuts[cut - 1u] {
                    boundaries[used] = cuts[cut];
                    used += 1u;
                }
            }
            if FOG_SCATTERING {
                optical_depth += profile_optical_depth(volume, ray, cuts);
            }
        }
    }
    sort_fog_boundaries(&boundaries, used);
    var state = FogIntegration(FogMedium(vec3<f32>(0.0), 1.0), 0u);
    let sample_mass = fog_sample_mass(optical_depth);
    for (var index = 1u; index < used; index += 1u) {
        let a = boundaries[index - 1u];
        let b = boundaries[index];
        if b <= a {
            continue;
        }
        var before = vec4<f32>(0.0);
        var after = vec4<f32>(0.0);
        var colors = 0u;
        for (var volume_index = 0u; volume_index < count; volume_index += 1u) {
            let hit = hits[volume_index];
            if hit.x <= a && hit.y >= b {
                let volume = globals.volumes[volume_index];
                let d0 = profile_density(volume, ray, a);
                let d1 = profile_density(volume, ray, b);
                before += vec4<f32>(volume.color.rgb, 1.0) * d0;
                after += vec4<f32>(volume.color.rgb, 1.0) * d1;
                colors += select(0u, 1u, d0 + d1 > 0.0);
            }
        }
        if before.a + after.a > 0.0 {
            integrate_profile_span(ray, vec2<f32>(a, b), before, after, colors, sample_mass, &state);
        }
    }
    return state.medium;
}
