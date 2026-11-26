
struct EmissionKey {
    position_time: vec4<f32>,
    rotation: vec4<f32>,
}

@group(0) @binding(10) var<storage, read> emission_keys: array<EmissionKey>;

fn birth_rotation(a: vec4<f32>, b: vec4<f32>, fraction: f32) -> vec4<f32> {
    let cosine = dot(a, b);
    let end_rotation = select(b, -b, cosine < 0.0);
    if abs(cosine) > 0.9995 {
        return normalize(mix(a, end_rotation, fraction));
    }
    let angle = acos(clamp(abs(cosine), 0.0, 1.0));
    let scale = 1.0 / sin(angle);
    return normalize(scale * (sin((1.0 - fraction) * angle) * a + sin(fraction * angle) * end_rotation));
}

fn birth_vector(rotation: vec4<f32>, vector: vec3<f32>) -> vec3<f32> {
    return vector + 2.0 * cross(rotation.xyz, cross(rotation.xyz, vector) + rotation.w * vector);
}

fn product_high(a: u32, b: u32) -> u32 {
    // Multiply 16-bit halves to obtain the high word of a 32×32 product.
    let low = (a & 65535u) * (b & 65535u);
    let middle = (a >> 16u) * (b & 65535u) + (low >> 16u);
    let carry = (middle & 65535u) + (a & 65535u) * (b >> 16u);
    return (a >> 16u) * (b >> 16u) + (middle >> 16u) + (carry >> 16u);
}

fn tracked_birth_time(particle: Particle, motion: Motion) -> f32 {
    // The integer birth ID stays fixed when new particles are emitted.
    let epoch = select(motion.parent_y, motion.parent_z, particle.tick > particle.emitter.indices.y);
    if motion.modes.w == 3u {
        let step = motion.parent_x.xy;
        let low = particle.tick * step.x;
        let sum = low + epoch.x;
        let high = particle.tick * step.y + product_high(particle.tick, step.x)
            + epoch.y + select(0u, 1u, sum < low);
        let period = bitcast<f32>(motion.parent_x.w);
        let upper = f32(high >> 16u) * (1.0 / 65536.0);
        let lower = f32(high & 65535u) * (1.0 / 4294967296.0) + f32(sum) * (1.0 / 18446744073709551616.0);
        let main = upper * period;
        let seconds = main + fma(lower, period, fma(upper, period, -main));
        return select(0.0, seconds, seconds < period);
    }
    return fma(f32(particle.tick), bitcast<f32>(motion.parent_x.w), bitcast<f32>(epoch.w));
}

fn tracked_particle_birth(particle: Particle, motion: Motion, origin: vec3<f32>, velocity: vec3<f32>) -> ParticleBirth {
    let time = tracked_birth_time(particle, motion);
    let count = arrayLength(&emission_keys);
    var low = 0u;
    var high = count;
    while low < high {
        let middle = low + (high - low) / 2u;
        if emission_keys[middle].position_time.w <= time {
            low = middle + 1u;
        } else {
            high = middle;
        }
    }
    let a = emission_keys[max(low, 1u) - 1u];
    let b = emission_keys[min(low, count - 1u)];
    var fraction = 0.0;
    if b.position_time.w > a.position_time.w {
        fraction = clamp((time - a.position_time.w) / (b.position_time.w - a.position_time.w), 0.0, 1.0);
    }
    let rotation = birth_rotation(a.rotation, b.rotation, fraction);
    let position = mix(a.position_time.xyz, b.position_time.xyz, fraction);
    let gravity = particle.emitter.gravity;
    var placed = particle;
    placed.emitter.gravity = vec4<f32>(birth_vector(rotation, gravity.xyz), gravity.w);
    return ParticleBirth(placed, position + birth_vector(rotation, origin), birth_vector(rotation, velocity));
}
