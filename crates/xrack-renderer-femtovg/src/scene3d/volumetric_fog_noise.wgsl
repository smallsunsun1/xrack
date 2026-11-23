// Periodic 3D value noise. A lattice cell maps to the user-specified world size.
fn fog_noise_corner(grid: vec3<u32>) -> f32 {
    let wrapped = grid & vec3<u32>(255u);
    var bits = wrapped.x | (wrapped.y << 8u) | (wrapped.z << 16u);
    bits += 0x9e3779b9u;
    bits = (bits ^ (bits >> 16u)) * 0x7feb352du;
    bits = (bits ^ (bits >> 15u)) * 0x846ca68bu;
    bits ^= bits >> 16u;
    return f32(bits & 65535u) / 65535.0;
}

fn fog_lattice_noise(phase: vec3<f32>) -> f32 {
    let point = 256.0 * fract(phase);
    let cell = vec3<u32>(floor(point));
    let f = fract(point);
    let blend = f * f * (vec3<f32>(3.0) - 2.0 * f);
    let x0 = mix(fog_noise_corner(cell), fog_noise_corner(cell + vec3<u32>(1, 0, 0)), blend.x);
    let x1 = mix(fog_noise_corner(cell + vec3<u32>(0, 1, 0)), fog_noise_corner(cell + vec3<u32>(1, 1, 0)), blend.x);
    let x2 = mix(fog_noise_corner(cell + vec3<u32>(0, 0, 1)), fog_noise_corner(cell + vec3<u32>(1, 0, 1)), blend.x);
    let x3 = mix(fog_noise_corner(cell + vec3<u32>(0, 1, 1)), fog_noise_corner(cell + vec3<u32>(1, 1, 1)), blend.x);
    return mix(mix(x0, x1, blend.y), mix(x2, x3, blend.y), blend.z);
}

fn fog_noise_multiplier(volume: FogVolume, world: vec3<f32>, step_length: f32) -> f32 {
    let phase = world * volume.noise.w - volume.noise.xyz;
    let footprint = step_length * volume.noise.w * 256.0;
    let coarse_weight = 1.0 - smoothstep(0.45, 1.4, footprint);
    let detail_weight = 1.0 - smoothstep(0.45, 1.4, 2.0 * footprint);
    var coarse = 0.5;
    var detail = 0.5;
    if coarse_weight > 0.0 {
        coarse = mix(0.5, fog_lattice_noise(phase), coarse_weight);
    }
    if detail_weight > 0.0 {
        detail = mix(0.5, fog_lattice_noise(phase * 2.0 + vec3<f32>(0.317, 0.073, 0.591)), detail_weight);
    }
    let noise = clamp((mix(coarse, detail, 0.32) - 0.24) / 0.52, 0.0, 1.0);
    return mix(1.0, noise, volume.color.w);
}

fn sample_fog_density(ray: FogRay, distance: f32, step_length: f32, mask: u32) -> vec4<f32> {
    let world = ray.origin + ray.direction * distance;
    var result = vec4<f32>(0.0);
    var left = mask;
    while left != 0u {
        let index = firstTrailingBit(left);
        left &= left - 1u;
        let volume = globals.volumes[index];
        var density = profile_density(volume, ray, distance);
        if density > 0.0 && volume.color.w > 0.0 {
            density *= fog_noise_multiplier(volume, world, step_length);
        }
        result += vec4<f32>(volume.color.rgb, 1.0) * density;
    }
    return result;
}

fn integrate_noisy_fog(ray: FogRay, clip: vec2<f32>, count: u32) -> FogMedium {
    var hits: array<vec2<f32>, 8>;
    var boundaries: array<f32, 32>;
    var used = 0u;
    var needs_noise = false;
    for (var index = 0u; index < count; index += 1u) {
        let volume = globals.volumes[index];
        let hit = fog_interval(volume, ray, clip);
        hits[index] = hit;
        if hit.y > hit.x {
            needs_noise = needs_noise || volume.color.w > 0.0;
            let cuts = profile_boundaries(volume, ray, hit);
            for (var cut = 0u; cut < 4u; cut += 1u) {
                if cut == 0u || cuts[cut] != cuts[cut - 1u] {
                    boundaries[used] = cuts[cut];
                    used += 1u;
                }
            }
        }
    }
    if used == 0u {
        return FogMedium(vec3<f32>(0.0), 1.0);
    }
    if !needs_noise {
        return integrate_fog_profiles(ray, clip, count);
    }
    sort_fog_boundaries(&boundaries, used);

    var masks: array<u32, 32>;
    var occupied = 0u;
    var total_length = 0.0;
    for (var span = 1u; span < used; span += 1u) {
        let left = boundaries[span - 1u];
        let right = boundaries[span];
        if right <= left {
            continue;
        }
        var mask = 0u;
        for (var index = 0u; index < count; index += 1u) {
            if hits[index].x <= left && hits[index].y >= right {
                mask |= 1u << index;
            }
        }
        masks[span] = mask;
        if mask != 0u {
            occupied += 1u;
            total_length += right - left;
        }
    }

    // Reserve a sample in every occupied interval, then distribute the
    // remaining budget over their lengths. Even distant thin volumes get a sample.
    let budget = max(occupied, u32(globals.volume_clip_count.w));
    let extra = budget - occupied;
    var extra_assigned = 0u;
    var traversed = 0.0;
    var state = FogIntegration(FogMedium(vec3<f32>(0.0), 1.0), 0u);
    // Only the lit specialization stores samples for a second integration sweep.
    // This lets the light integrator reuse the same opacity-stratified budget.
    var sample_values: array<vec4<f32>, 64>;
    var sample_counts: array<u32, 32>;
    var sample_index = 0u;
    var optical_depth = 0.0;
    for (var span = 1u; span < used; span += 1u) {
        let mask = masks[span];
        if mask == 0u {
            continue;
        }
        let left = boundaries[span - 1u];
        let span_length = boundaries[span] - left;
        traversed += span_length;
        let new_assigned = min(extra, u32(f32(extra) * (traversed / total_length)));
        let steps = 1u + new_assigned - extra_assigned;
        extra_assigned = new_assigned;
        let step_length = span_length / f32(steps);
        for (var cell = 0u; cell < steps; cell += 1u) {
            let near = fma(f32(cell), step_length, left);
            let medium = sample_fog_density(ray, near + 0.5 * step_length, step_length, mask);
            if FOG_SCATTERING {
                sample_values[sample_index] = medium;
                sample_index += 1u;
                optical_depth += medium.a * step_length;
            } else if medium.a > 0.0 {
                integrate_fog_segment(ray, near, step_length, medium.a, medium.rgb / medium.a, 0.0, &state);
            }
        }
        if FOG_SCATTERING {
            sample_counts[span] = steps;
        }
    }
    if FOG_SCATTERING {
        let mass = fog_sample_mass(optical_depth);
        sample_index = 0u;
        for (var span = 1u; span < used; span += 1u) {
            let steps = sample_counts[span];
            if steps == 0u {
                continue;
            }
            let left = boundaries[span - 1u];
            let step_length = (boundaries[span] - left) / f32(steps);
            for (var cell = 0u; cell < steps; cell += 1u) {
                let medium = sample_values[sample_index];
                sample_index += 1u;
                if medium.a > 0.0 {
                    let near = fma(f32(cell), step_length, left);
                    integrate_fog_segment(ray, near, step_length, medium.a, medium.rgb / medium.a, mass, &state);
                }
            }
        }
    }
    return state.medium;
}
