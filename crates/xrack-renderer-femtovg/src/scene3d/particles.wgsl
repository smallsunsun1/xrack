struct Emitter {
    position_radius: vec4<f32>,
    velocity_end_radius: vec4<f32>,
    spread_lifetime: vec4<f32>,
    gravity: vec4<f32>,
    color: vec4<f32>,
    timing: vec4<f32>,
    // Exclusive end in the instance range, latest birth ID, padding, seed.
    indices: vec4<u32>,
    // Frame count, duration, duration variation, and initial offset.
    sequence: vec4<f32>,
    // U stride per cell, half texel in U/V, and width / height of one frame.
    sheet: vec4<f32>,
    // Playback mode, interpolation flag, random start flag, and affector mask.
    playback: vec4<u32>,
    // Lifetime spread, birth and end radius spread. A negative end spread shares the birth sample.
    birth_variation: vec4<f32>,
    // Initial angle, initial angle spread, spin and spin spread around the view axis.
    billboard_rotation: vec4<f32>,
}

struct View {
    clip_x: vec4<f32>,
    clip_y: vec4<f32>,
    clip_z: vec4<f32>,
    clip_w: vec4<f32>,
    view_depth: vec4<f32>,
    lens: vec4<f32>,
    lateral_x: vec4<f32>,
    lateral_y: vec4<f32>,
    emitters: array<Emitter, 16>,
}

struct Motion {
    shape_x: vec4<f32>,
    shape_y: vec4<f32>,
    shape_z: vec4<f32>,
    surface_weights: vec4<f32>,
    axis_speed: vec4<f32>,
    right_cosine: vec4<f32>,
    up_variation: vec4<f32>,
    target_point: vec4<f32>,
    target_spread: vec4<f32>,
    // Shape, surface, velocity mode, and presence of a scene attachment.
    modes: vec4<u32>,
    // Affine rows or the tracked birth clock, according to modes.w.
    parent_x: vec4<u32>,
    parent_y: vec4<u32>,
    parent_z: vec4<u32>,
}

struct Motions {
    emitters: array<Motion, 16>,
}

struct SurfaceTriangle {
    a: vec3<f32>,
    probability: f32,
    b: vec3<f32>,
    alternate: u32,
    c: vec3<f32>,
    normal: vec3<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var sheet_texture: texture_2d<f32>;
@group(0) @binding(2) var sheet_sampler: sampler;
@group(0) @binding(3) var color_table: texture_2d<f32>;
@group(0) @binding(4) var<uniform> motions: Motions;
@group(0) @binding(5) var<storage, read> surface_triangles: array<SurfaceTriangle>;

struct Affector {
    position_age: vec4<f32>,
    vector_value: vec4<f32>,
    time_bounds: vec4<f32>,
    control: vec4<u32>,
    world_to_local: array<vec4<f32>, 3>,
    local_to_world: array<vec4<f32>, 3>,
}

struct Wander {
    unique_amount: vec4<f32>,
    unique_pace: vec4<f32>,
}

struct Affectors {
    fields: array<Affector, 16>,
    wander: array<Wander, 16>,
}

@group(0) @binding(6) var<uniform> affectors: Affectors;

struct Trails {
    // Duration, end scale, opacity loss, end-of-life fade.
    settings: array<vec4<f32>, 16>,
    segment_count: vec4<u32>,
}

@group(0) @binding(7) var<uniform> trails: Trails;
override TEXTURED: bool = false;

struct Sprite {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) color: vec4<f32>,
    @location(2) @interpolate(flat) depth: f32,
    @location(3) @interpolate(flat) sheet: vec4<f32>,
    @location(4) @interpolate(flat) frames: vec3<f32>,
    // XRack wireframe varying
    // XRack linked sample varying
}

struct SpriteVertex {
    sprite: Sprite,
    age: f32,
    seed: u32,
}

struct Particle {
    emitter: Emitter,
    slot: u32,
    seconds: f32,
    seed: u32,
    tick: u32,
    billboard_angle: f32,
}

fn random01(input: u32) -> f32 {
    var bits = (input ^ (input >> 16u)) * 0x7feb352du;
    bits = (bits ^ (bits >> 15u)) * 0x846ca68bu;
    return f32((bits ^ (bits >> 16u)) >> 8u) / 16777216.0;
}

fn signed_random(seed: u32) -> f32 {
    return 2.0 * random01(seed) - 1.0;
}

fn random3(seed: u32) -> vec3<f32> {
    return vec3<f32>(random01(seed), random01(seed ^ 0xc13fa9a9u), random01(seed ^ 0x91e10da5u));
}

fn circle(sample: f32) -> vec2<f32> {
    let angle = sample * 6.28318530718;
    return vec2<f32>(cos(angle), sin(angle));
}

fn unit_sphere(pick: vec2<f32>) -> vec3<f32> {
    let height = 2.0 * pick.x - 1.0;
    let ring = circle(pick.y) * sqrt(max(0.0, 1.0 - height * height));
    return vec3<f32>(ring.x, height, ring.y);
}

fn ellipsoid(shape: Motion, seed: u32, pick: vec3<f32>) -> vec3<f32> {
    var outward = unit_sphere(pick.xy);
    if shape.modes.y == 0u {
        return outward * pow(pick.z, 1.0 / 3.0);
    }
    var acceptance = pick.z;
    // The probability compensates area stretch under non-uniform scale.
    for (var attempt = 1u; attempt < 12u; attempt++) {
        if acceptance <= length(outward * shape.surface_weights.xyz) {
            break;
        }
        let next = random3(seed + attempt * 0x9e3779b9u);
        outward = unit_sphere(next.xy);
        acceptance = next.z;
    }
    return outward;
}

fn cylinder(shape: Motion, seed: u32, pick: vec3<f32>) -> vec3<f32> {
    var around = circle(pick.x);
    let height = 2.0 * pick.y - 1.0;
    if shape.modes.y == 0u {
        let disk = around * sqrt(pick.z);
        return vec3<f32>(disk.x, height, disk.y);
    }
    let cap = random01(seed ^ 0x94d049bbu) < shape.shape_z.w;
    if cap {
        let disk = around * sqrt(pick.z);
        return vec3<f32>(disk.x, select(-1.0, 1.0, height >= 0.0), disk.y);
    }
    var acceptance = pick.z;
    for (var attempt = 1u; attempt < 12u; attempt++) {
        if acceptance <= length(around.yx * shape.surface_weights.xy) {
            break;
        }
        let next = random3(seed + attempt * 0x9e3779b9u);
        around = circle(next.x);
        acceptance = next.z;
    }
    return vec3<f32>(around.x, height, around.y);
}

fn spawn_offset(shape: Motion, particle_seed: u32) -> vec3<f32> {
    if shape.modes.x == 0u {
        return vec3<f32>(0.0);
    }
    let seed = particle_seed ^ 0xd1b54a35u;
    let pick = random3(seed);
    var local = pick * 2.0 - 1.0;
    switch shape.modes.x {
        case 1u: {
            if shape.modes.y != 0u {
                let face = random01(seed ^ 0x85ebca6bu);
                if face < shape.shape_x.w {
                    local.x = select(-1.0, 1.0, pick.x >= 0.5);
                } else if face < shape.shape_y.w {
                    local.y = select(-1.0, 1.0, pick.y >= 0.5);
                } else {
                    local.z = select(-1.0, 1.0, pick.z >= 0.5);
                }
            }
        }
        case 2u: {
            local = ellipsoid(shape, seed, pick);
        }
        case 3u: {
            local = cylinder(shape, seed, pick);
        }
        default: {}
    }
    return shape_vector(shape, local);
}

fn shape_vector(shape: Motion, local: vec3<f32>) -> vec3<f32> {
    return shape.shape_x.xyz * local.x + shape.shape_y.xyz * local.y + shape.shape_z.xyz * local.z;
}

fn normalized_or(value: vec3<f32>, fallback: vec3<f32>) -> vec3<f32> {
    let squared = dot(value, value);
    if squared > 0.0 {
        return value * inverseSqrt(squared);
    }
    return fallback;
}

fn vector_velocity(emitter: Emitter, seed: u32) -> vec3<f32> {
    return emitter.velocity_end_radius.xyz + (2.0 * random3(seed) - 1.0) * emitter.spread_lifetime.xyz;
}

fn spatial_velocity(particle: Particle, motion: Motion, origin: vec3<f32>, normal: vec3<f32>) -> vec3<f32> {
    if motion.modes.z == 0u {
        return vector_velocity(particle.emitter, particle.seed);
    }
    let pick = random3(particle.seed ^ 0x632be59bu);
    let speed_variation = 2.0 * random01(particle.seed ^ 0x27d4eb2du) - 1.0;
    let speed = max(0.0, motion.axis_speed.w + speed_variation * motion.up_variation.w);
    let axis = motion.axis_speed.xyz;
    switch motion.modes.z {
        case 1u: {
            let cosine = mix(motion.right_cosine.w, 1.0, pick.x);
            let ring = circle(pick.y) * sqrt(max(0.0, 1.0 - cosine * cosine));
            return speed * (axis * cosine + motion.right_cosine.xyz * ring.x + motion.up_variation.xyz * ring.y);
        }
        case 2u: {
            let destination = motion.target_point.xyz + (2.0 * pick - 1.0) * motion.target_spread.xyz;
            return speed * normalized_or(destination - origin, axis);
        }
        case 4u: {
            return speed * normal;
        }
        default: {
            return speed * normalized_or(vector_velocity(particle.emitter, particle.seed), axis);
        }
    }
}

fn frame_at(tick: u32, count: u32, mode: u32) -> f32 {
    if mode == 4u {
        return f32(min(tick, count - 1u));
    }
    let ping_pong = mode == 2u || mode == 3u;
    let period = select(count, 2u * (count - 1u), ping_pong);
    var index = tick % period;
    if ping_pong {
        index = min(index, period - index);
    }
    if mode == 1u || mode == 3u {
        index = count - 1u - index;
    }
    return f32(index);
}

fn sequence_frames(emitter: Emitter, age: f32, seed: u32) -> vec3<f32> {
    let count = u32(emitter.sequence.x);
    if count <= 1u {
        return vec3<f32>(0.0);
    }
    let mode = emitter.playback.x;
    let random_offset = floor(random01(seed ^ 0x3c6ef372u) * emitter.sequence.x);
    let start = select(emitter.sequence.w, random_offset, emitter.playback.z != 0u);
    if mode == 5u {
        return vec3<f32>(start, start, 0.0);
    }
    let speed = 2.0 * random01(seed ^ 0xbb67ae85u) - 1.0;
    let nominal_duration = select(emitter.sequence.y, emitter.spread_lifetime.w, emitter.sequence.y == 0.0);
    let duration = max(nominal_duration + speed * emitter.sequence.z, 1e-6);
    let phase = age / duration;
    let ping_pong = mode == 2u || mode == 3u;
    let span = f32(select(count, 2u * (count - 1u), ping_pong));
    let position = select(fract(phase), min(phase, 1.0), mode == 4u) * span + start;
    let tick = u32(position);
    let blend = select(0.0, fract(position), emitter.playback.y != 0u);
    return vec3<f32>(frame_at(tick, count, mode), frame_at(tick + 1u, count, mode), blend);
}

fn particle_at(index: u32) -> Particle {
    var slot = 0u;
    while slot + 1u < 16u && index >= view.emitters[slot].indices.x {
        slot++;
    }
    var emitter = view.emitters[slot];
    let reverse = emitter.indices.x - index - 1u;
    let tick = emitter.indices.y - reverse;
    let age = emitter.timing.y + f32(reverse) * emitter.timing.x;
    let seed = emitter.indices.w ^ (tick * 0xa24baed5u);
    let variation = emitter.birth_variation;
    if variation.x > 0.0 {
        emitter.spread_lifetime.w = clamp(
            emitter.spread_lifetime.w + signed_random(seed ^ 0xb5297a4du) * variation.x,
            1e-6, 1e8);
    }
    if variation.y > 0.0 || variation.z > 0.0 {
        let initial_delta = signed_random(seed ^ 0x68e31da4u) * variation.y;
        var final_delta = initial_delta;
        if variation.z >= 0.0 {
            final_delta = signed_random(seed ^ 0x1b56c4e9u) * variation.z;
        }
        emitter.position_radius.w = clamp(emitter.position_radius.w + initial_delta, 0.0, 5e7);
        emitter.velocity_end_radius.w = clamp(emitter.velocity_end_radius.w + final_delta, 0.0, 5e7);
    }
    let roll = emitter.billboard_rotation;
    var billboard_angle = 0.0;
    if any(roll != vec4<f32>(0.0)) {
        let start = roll.x + signed_random(seed ^ 0x3141592du ^ 0x91e10da5u) * roll.y;
        let spin = roll.z + signed_random(seed ^ 0x75d77a21u ^ 0x91e10da5u) * roll.w;
        billboard_angle = fract((start + age * spin) / 360.0) * 6.28318530718;
    }
    return Particle(emitter, slot, age, seed, tick, billboard_angle);
}

struct ParticleCenter {
    particle: Particle,
    center: vec3<f32>,
}

struct ParticleBirth {
    particle: Particle,
    origin: vec3<f32>,
    velocity: vec3<f32>,
}

fn trajectory(birth: ParticleBirth, age: f32) -> ParticleCenter {
    var particle = birth.particle;
    particle.seconds = age;
    let center = birth.origin + age * (birth.velocity + (0.5 * age) * particle.emitter.gravity.xyz);
    return ParticleCenter(particle, center);
}

fn present_position(birth: ParticleBirth) -> ParticleCenter {
    return trajectory(birth, birth.particle.seconds);
}

fn base_alpha(emitter: Emitter, age: f32) -> f32 {
    let fade_in = select(1.0, min(age / max(emitter.timing.z, 1e-12), 1.0), emitter.timing.z > 0.0);
    let fade_out = select(1.0, clamp((emitter.spread_lifetime.w - age) / max(emitter.timing.w, 1e-12), 0.0, 1.0), emitter.timing.w > 0.0);
    return emitter.color.a * fade_in * fade_out;
}

fn billboard_offset(offset: vec2<f32>) -> vec4<f32> {
    if view.lens.w > 0.5 {
        return view.lateral_x * offset.x + view.lateral_y * offset.y;
    }
    return vec4<f32>(offset * view.lens.xy, 0.0, 0.0);
}

fn sprite_vertex(vertex: u32, source: ParticleCenter, in_field: bool) -> SpriteVertex {
    let emitter = source.particle.emitter;
    let age = source.particle.seconds;
    let seed = source.particle.seed;
    let center = source.center;
    let lifetime = emitter.spread_lifetime.w;
    let visible = in_field && age < lifetime;
    let age_fraction = clamp(age / lifetime, 0.0, 1.0);
    let radius = select(0.0, mix(emitter.position_radius.w, emitter.velocity_end_radius.w, age_fraction), visible);
    let corners = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, 1.0),
    );
    let uv = corners[vertex];
    let world = vec4<f32>(center, 1.0);
    let xy = vec2<f32>(dot(view.clip_x, world), dot(view.clip_y, world));
    var aspect = 1.0;
    var frames = vec3<f32>(0.0);
    if TEXTURED {
        aspect = emitter.sheet.w;
        frames = sequence_frames(emitter, age, seed);
    }
    var coordinates = uv * vec2<f32>(aspect, 1.0) * radius;
    let angle = source.particle.billboard_angle;
    if angle != 0.0 {
        let c = cos(angle);
        let s = sin(angle);
        coordinates = vec2<f32>(c * coordinates.x - s * coordinates.y, s * coordinates.x + c * coordinates.y);
    }
    let offset = billboard_offset(coordinates);
    let clip = vec4<f32>(xy, dot(view.clip_z, world), dot(view.clip_w, world)) + offset;
    let alpha = select(0.0, base_alpha(emitter, age), visible);
    let sprite = Sprite(clip, uv, vec4<f32>(emitter.color.rgb * alpha, alpha),
        dot(view.view_depth, world), emitter.sheet, frames,
        // XRack wireframe corner
        // XRack linked clip depth
    );
    return SpriteVertex(sprite, age_fraction, seed);
}

@vertex
fn vertex_sprite(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return sprite_vertex(vertex, present_position(vector_source(index)), true).sprite;
}

fn vector_source(index: u32) -> ParticleBirth {
    let particle = particle_at(index);
    return ParticleBirth(particle, particle.emitter.position_radius.xyz,
        vector_velocity(particle.emitter, particle.seed));
}

fn place_particle_birth(particle: Particle, motion: Motion, origin: vec3<f32>, velocity: vec3<f32>) -> ParticleBirth {
    // XRack particle birth track
    if motion.modes.w == 0u {
        return ParticleBirth(particle, origin, velocity);
    }
    let frame = transpose(mat3x4<f32>(
        bitcast<vec4<f32>>(motion.parent_x), bitcast<vec4<f32>>(motion.parent_y), bitcast<vec4<f32>>(motion.parent_z)));
    let gravity = particle.emitter.gravity;
    var placed = particle;
    placed.emitter.gravity = vec4<f32>(frame * vec4<f32>(gravity.xyz, 0.0), gravity.w);
    return ParticleBirth(placed, frame * vec4<f32>(origin, 1.0), frame * vec4<f32>(velocity, 0.0));
}

fn spatial_source(index: u32) -> ParticleBirth {
    let particle = particle_at(index);
    let motion = motions.emitters[particle.slot];
    let origin = particle.emitter.position_radius.xyz + spawn_offset(motion, particle.seed);
    return place_particle_birth(particle, motion, origin, spatial_velocity(particle, motion, origin, vec3<f32>(0.0)));
}

fn mesh_source(index: u32) -> ParticleBirth {
    let particle = particle_at(index);
    let count = arrayLength(&surface_triangles);
    let seed = particle.seed ^ 0x3cd9790du;
    let pick = random3(seed);
    let column = min(u32(pick.x * f32(count)), count - 1u);
    let entry = surface_triangles[column];
    let selected = select(entry.alternate, column, pick.y < entry.probability);
    let triangle = surface_triangles[selected];
    // Square-root barycentric mapping gives uniform area across a triangle.
    let radius = sqrt(pick.z);
    let u = random01(seed ^ 0x74b0dc51u);
    let local = (1.0 - radius) * triangle.a + radius * mix(triangle.b, triangle.c, u);
    let motion = motions.emitters[particle.slot];
    let origin = particle.emitter.position_radius.xyz + shape_vector(motion, local);
    let normal = shape_vector(motion, triangle.normal);
    return place_particle_birth(particle, motion, origin, spatial_velocity(particle, motion, origin, normal));
}

fn tinted(input: SpriteVertex) -> Sprite {
    var sprite = input.sprite;
    let size = vec2<f32>(textureDimensions(color_table));
    let row = floor(random01(input.seed ^ 0x510e527fu) * size.y);
    let texel = vec2<f32>(0.5 + input.age * (size.x - 1.0), row + 0.5);
    sprite.color *= textureSampleLevel(color_table, sheet_sampler, texel / size, 0.0);
    return sprite;
}

@vertex
fn vertex_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return tinted(sprite_vertex(vertex, present_position(vector_source(index)), true));
}

@vertex
fn vertex_spatial(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return sprite_vertex(vertex, present_position(spatial_source(index)), true).sprite;
}

@vertex
fn vertex_spatial_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return tinted(sprite_vertex(vertex, present_position(spatial_source(index)), true));
}

@vertex
fn vertex_mesh(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return sprite_vertex(vertex, present_position(mesh_source(index)), true).sprite;
}

@vertex
fn vertex_mesh_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return tinted(sprite_vertex(vertex, present_position(mesh_source(index)), true));
}

struct Displaced {
    source: ParticleCenter,
    visible: bool,
}

fn affector_world_vector(field: Affector, delta: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        dot(field.local_to_world[0].xyz, delta),
        dot(field.local_to_world[1].xyz, delta),
        dot(field.local_to_world[2].xyz, delta));
}

fn wander_offset(field: Affector, extra: Wander, particle: Particle, age: f32) -> vec3<f32> {
    let tau = 6.28318530718;
    let envelope = min(
        select(1.0, clamp(age / max(field.vector_value.w, 1e-12), 0.0, 1.0), field.vector_value.w > 0.0),
        select(1.0, clamp((particle.emitter.spread_lifetime.w - particle.seconds) /
            max(field.time_bounds.w, 1e-12), 0.0, 1.0), field.time_bounds.w > 0.0));
    var delta = field.vector_value.xyz * sin(tau * fract(field.position_age.xyz + age * field.time_bounds.xyz));
    if any(extra.unique_amount.xyz != vec3<f32>(0.0)) {
        let seed = field.control.y ^ particle.seed;
        let phase = random3(seed ^ 0x3f94bb27u);
        let jitter_amount = 1.0 + extra.unique_amount.w * (2.0 * random3(seed ^ 0xdc3cf22bu) - 1.0);
        let jitter_pace = 1.0 + extra.unique_pace.w * (2.0 * random3(seed ^ 0xd986e9c5u) - 1.0);
        let angle = tau * fract(phase + age * extra.unique_pace.xyz * jitter_pace);
        delta += extra.unique_amount.xyz * jitter_amount * sin(angle);
    }
    return envelope * delta;
}

fn scale_easing(t: f32, easing: u32) -> f32 {
    let reverse = 1.0 - t;
    switch easing {
        case 1u: { return t * t * t; }
        case 2u: { return 1.0 - reverse * reverse * reverse; }
        case 3u: { return select(1.0 - 4.0 * reverse * reverse * reverse, 4.0 * t * t * t, t < 0.5); }
        default: { return t; }
    }
}

fn scale_factor(field: Affector, age: f32) -> f32 {
    let t = fract(age / field.vector_value.z);
    let easing = field.control.z >> 3u;
    var ratio: f32;
    switch field.control.z & 7u {
        case 1u: {
            let up = t < 0.5;
            let progress = scale_easing(fract(2.0 * t), easing);
            ratio = select(1.0 - progress, progress, up);
        }
        case 2u: { ratio = 0.5 + 0.5 * sin(6.28318530718 * t); }
        case 3u: { ratio = abs(sin(6.28318530718 * t)); }
        case 4u: { ratio = step(0.5, t); }
        case 5u: { ratio = smoothstep(0.0, 1.0, t); }
        default: { ratio = scale_easing(t, easing); }
    }
    return mix(field.vector_value.x, field.vector_value.y, clamp(ratio, 0.0, 1.0));
}

fn apply_affectors(original: ParticleCenter) -> Displaced {
    var source = original;
    var visible = true;
    var scale = 1.0;
    let particle = source.particle;
    var mask = particle.emitter.playback.w;
    while mask != 0u {
        let bit = firstTrailingBit(mask);
        mask &= mask - 1u;
        let field = affectors.fields[bit];
        let age = particle.seconds - field.position_age.w;
        if age < 0.0 {
            continue;
        }
        let attached = field.control.w != 0u;
        if field.control.x == 5u {
            let offset = wander_offset(field, affectors.wander[bit], particle, age);
            if attached {
                source.center += affector_world_vector(field, offset);
            } else {
                source.center += offset;
            }
            continue;
        }
        if field.control.x == 6u {
            scale *= scale_factor(field, age);
            continue;
        }
        if attached && field.control.x == 3u {
            let acceleration = affector_world_vector(field, field.vector_value.xyz);
            source.center += acceleration * (0.5 * age * age);
            continue;
        }
        var center = source.center;
        if attached {
            let point = vec4<f32>(center, 1.0);
            center = vec3<f32>(
                dot(field.world_to_local[0], point),
                dot(field.world_to_local[1], point),
                dot(field.world_to_local[2], point));
        }
        let before = center;
        let position = field.position_age.xyz;
        switch field.control.x {
            case 1u: {
                let seed = particle.seed ^ field.control.y ^ 0xd2b74407u;
                let remaining_life = particle.emitter.spread_lifetime.w - field.position_age.w;
                let duration = select(field.time_bounds.x, remaining_life, field.time_bounds.x < 0.0);
                let varied = duration + (2.0 * random01(seed ^ 0x94d049bbu) - 1.0) * field.time_bounds.y;
                let progress = clamp(age / max(varied, 1e-6), 0.0, 1.0);
                let destination = position + (2.0 * random3(seed) - 1.0) * field.vector_value.xyz;
                center = mix(center, destination, progress);
                visible = visible && !(field.control.z != 0u && progress >= 1.0);
            }
            case 2u: {
                let delta = center - position;
                let distance = length(delta);
                let outer = field.time_bounds.w;
                if distance > 0.0 && distance < outer {
                    let inner = field.time_bounds.z;
                    var attenuation = 1.0;
                    if distance > inner {
                        attenuation = 1.0 - smoothstep(inner, outer, distance);
                    }
                    center += (delta / distance) * (field.vector_value.w * attenuation);
                }
            }
            case 3u: {
                center += field.vector_value.xyz * (0.5 * age * age);
            }
            case 4u: {
                let axis = field.vector_value.xyz;
                let angle = 6.28318530718 * fract(age * field.vector_value.w / 360.0);
                let cosine = cos(angle);
                let sine = sin(angle);
                let delta = center - position;
                center = position + delta * cosine + cross(axis, delta) * sine
                    + axis * dot(axis, delta) * (1.0 - cosine);
            }
            default: {}
        }
        if attached {
            // Keep the original world position exact for particles outside the field.
            source.center += affector_world_vector(field, center - before);
        } else {
            source.center = center;
        }
    }
    if scale != 1.0 {
        scale = min(scale, 4096.0);
        source.particle.emitter.position_radius.w *= scale;
        source.particle.emitter.velocity_end_radius.w *= scale;
    }
    return Displaced(source, visible && scale > 0.0);
}

fn affected_sprite(vertex: u32, source: ParticleCenter) -> SpriteVertex {
    let displaced = apply_affectors(source);
    return sprite_vertex(vertex, displaced.source, displaced.visible);
}

@vertex
fn vertex_affected(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return affected_sprite(vertex, present_position(vector_source(index))).sprite;
}

@vertex
fn vertex_affected_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return tinted(affected_sprite(vertex, present_position(vector_source(index))));
}

@vertex
fn vertex_spatial_affected(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return affected_sprite(vertex, present_position(spatial_source(index))).sprite;
}

@vertex
fn vertex_spatial_affected_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return tinted(affected_sprite(vertex, present_position(spatial_source(index))));
}

@vertex
fn vertex_mesh_affected(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return affected_sprite(vertex, present_position(mesh_source(index))).sprite;
}

@vertex
fn vertex_mesh_affected_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Sprite {
    return tinted(affected_sprite(vertex, present_position(mesh_source(index))));
}

struct Ribbon {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) depth: f32,
    @location(3) @interpolate(flat) sheet: vec4<f32>,
    @location(4) @interpolate(flat) frames: vec3<f32>,
    // XRack wireframe varying
    // XRack linked sample varying
}

fn ribbon(sprite: Sprite) -> Ribbon {
    return Ribbon(sprite.position, sprite.uv, sprite.color, sprite.depth, sprite.sheet, sprite.frames,
        // XRack wireframe ribbon
        // XRack linked ribbon depth
    );
}

struct TrailInterval {
    age: f32,
    younger: f32,
    older: f32,
    head: f32,
    fraction: f32,
    end_opacity: f32,
}

fn trail_interval(vertex: u32, particle: Particle) -> TrailInterval {
    let settings = trails.settings[particle.slot];
    let life = particle.emitter.spread_lifetime.w;
    let head = min(particle.seconds, life);
    let span = min(head, settings.x);
    let segments = trails.segment_count.x;
    let point = vertex / 2u;
    let count = f32(segments);
    let fraction = f32(point) / count;
    let younger = f32(max(point, 1u) - 1u) / count;
    let older = f32(min(point + 1u, segments)) / count;
    let elapsed = particle.seconds - life;
    let end_opacity = select(select(0.0, 1.0, elapsed < 0.0),
        1.0 - clamp(elapsed / max(settings.w, 1e-20), 0.0, 1.0), settings.w > 0.0);
    return TrailInterval(head - span * fraction, head - span * younger, head - span * older,
        head, fraction, end_opacity);
}

fn trail_clip(center: vec3<f32>) -> vec4<f32> {
    let world = vec4<f32>(center, 1.0);
    return vec4<f32>(dot(view.clip_x, world), dot(view.clip_y, world),
        dot(view.clip_z, world), dot(view.clip_w, world));
}

fn projected_position(clip: vec4<f32>) -> vec2<f32> {
    if view.lens.w > 0.5 {
        return clip.xy / max(clip.w, 1e-5);
    }
    // A tangent in units with the camera's aspect taken out keeps ribbon
    // width invariant as the viewport changes size.
    return clip.xy / (max(clip.w, 1e-5) * view.lens.xy);
}

fn trail_vertex(vertex: u32, sample: Displaced, interval: TrailInterval,
                younger: vec3<f32>, older: vec3<f32>) -> SpriteVertex {
    let particle = sample.source.particle;
    let emitter = particle.emitter;
    let settings = trails.settings[particle.slot];
    let life_fraction = clamp(interval.age / emitter.spread_lifetime.w, 0.0, 1.0);
    let radius = mix(emitter.position_radius.w, emitter.velocity_end_radius.w, life_fraction)
        * mix(1.0, settings.y, interval.fraction);
    let clip = trail_clip(sample.source.center);
    let headward = projected_position(trail_clip(younger));
    let tailward = projected_position(trail_clip(older));
    var tangent = tailward - headward;
    if all(tangent == vec2<f32>(0.0)) {
        tangent = tailward - projected_position(clip);
    }
    if view.lens.w > 0.5 {
        let ndc = projected_position(clip);
        let a = view.lateral_x.xy - ndc * view.lateral_x.w;
        let b = view.lateral_y.xy - ndc * view.lateral_y.w;
        let det = a.x * b.y - a.y * b.x;
        if abs(det) > 1e-12 {
            tangent = vec2<f32>(b.y * tangent.x - b.x * tangent.y,
                a.x * tangent.y - a.y * tangent.x) / det;
        }
    }
    let magnitude = max(abs(tangent.x), abs(tangent.y));
    var edge = vec2<f32>(0.0);
    if magnitude > 1e-12 {
        let direction = normalize(tangent / magnitude);
        edge = vec2<f32>(-direction.y, direction.x);
    }
    let side = select(-1.0, 1.0, (vertex & 1u) != 0u);
    let offset_clip = clip + billboard_offset(edge * side * radius);
    var frames = vec3<f32>(0.0);
    if TEXTURED {
        frames = sequence_frames(emitter, interval.head, particle.seed);
    }
    let alpha = select(0.0, base_alpha(emitter, interval.age)
        * (1.0 - settings.z * interval.fraction) * interval.end_opacity, sample.visible);
    let world = vec4<f32>(sample.source.center, 1.0);
    let sprite = Sprite(offset_clip, vec2<f32>(side, 1.0 - 2.0 * interval.fraction),
        vec4<f32>(emitter.color.rgb * alpha, alpha), dot(view.view_depth, world), emitter.sheet, frames,
        // XRack wireframe corner
        // XRack linked offset clip depth
    );
    return SpriteVertex(sprite, life_fraction, particle.seed);
}

fn plain_trail(vertex: u32, birth: ParticleBirth) -> SpriteVertex {
    let interval = trail_interval(vertex, birth.particle);
    return trail_vertex(vertex, Displaced(trajectory(birth, interval.age), true), interval,
        trajectory(birth, interval.younger).center, trajectory(birth, interval.older).center);
}

fn affected_trail(vertex: u32, birth: ParticleBirth) -> SpriteVertex {
    let interval = trail_interval(vertex, birth.particle);
    return trail_vertex(vertex, apply_affectors(trajectory(birth, interval.age)), interval,
        apply_affectors(trajectory(birth, interval.younger)).source.center,
        apply_affectors(trajectory(birth, interval.older)).source.center);
}

@vertex
fn vertex_trail(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(plain_trail(vertex, vector_source(index)).sprite);
}

@vertex
fn vertex_trail_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(plain_trail(vertex, vector_source(index))));
}

@vertex
fn vertex_trail_affected(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(affected_trail(vertex, vector_source(index)).sprite);
}

@vertex
fn vertex_trail_affected_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(affected_trail(vertex, vector_source(index))));
}

@vertex
fn vertex_spatial_trail(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(plain_trail(vertex, spatial_source(index)).sprite);
}

@vertex
fn vertex_spatial_trail_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(plain_trail(vertex, spatial_source(index))));
}

@vertex
fn vertex_spatial_trail_affected(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(affected_trail(vertex, spatial_source(index)).sprite);
}

@vertex
fn vertex_spatial_trail_affected_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(affected_trail(vertex, spatial_source(index))));
}

@vertex
fn vertex_mesh_trail(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(plain_trail(vertex, mesh_source(index)).sprite);
}

@vertex
fn vertex_mesh_trail_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(plain_trail(vertex, mesh_source(index))));
}

@vertex
fn vertex_mesh_trail_affected(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(affected_trail(vertex, mesh_source(index)).sprite);
}

@vertex
fn vertex_mesh_trail_affected_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(affected_trail(vertex, mesh_source(index))));
}

fn trail_color(sprite: Ribbon) -> vec4<f32> {
    // XRack wireframe discard
    let coverage = 1.0 - smoothstep(0.35, 1.0, abs(sprite.uv.x));
    return sprite.color * coverage;
}

@fragment
fn fragment_trail(sprite: Ribbon) -> @location(0) vec4<f32> {
    return trail_color(sprite);
}

@fragment
fn fragment_trail_bounded(sprite: Ribbon) -> @location(0) vec4<f32> {
    return bounded(trail_color(sprite));
}

@fragment
fn fragment_trail_weighted(sprite: Ribbon /* XRack linked sample */) -> WeightedColor {
    let color = trail_color(sprite);
    // XRack linked fragment sprite.position
    return weighted_color(color, sprite.depth);
}

@fragment
fn fragment_sprite(sprite: Sprite) -> @location(0) vec4<f32> {
    return sprite_color(sprite);
}

fn sprite_color(sprite: Sprite) -> vec4<f32> {
    // XRack wireframe discard
    let coverage = 1.0 - smoothstep(0.15, 1.0, length(sprite.uv));
    return sprite.color * coverage;
}

fn sprite_sample(coordinates: vec2<f32>, sheet: vec4<f32>, frames: vec3<f32>) -> vec4<f32> {
    let cell = sheet.x;
    let inset = sheet.yz;
    let local = (vec2<f32>(0.5) + coordinates * vec2<f32>(0.5, -0.5)) * vec2<f32>(cell, 1.0);
    // Clamp each lookup to the active frame's texel centers, including 1px cells.
    let uv = clamp(local, inset, max(inset, vec2<f32>(cell, 1.0) - inset));
    let first = textureSampleLevel(sheet_texture, sheet_sampler, uv + vec2<f32>(frames.x * cell, 0.0), 0.0);
    var color = first;
    if frames.z > 0.0 && frames.x != frames.y {
        let second = textureSampleLevel(sheet_texture, sheet_sampler, uv + vec2<f32>(frames.y * cell, 0.0), 0.0);
        color = mix(first, second, frames.z);
    }
    return color;
}

fn texture_color(sprite: Sprite) -> vec4<f32> {
    // XRack wireframe discard
    return sprite_sample(sprite.uv, sprite.sheet, sprite.frames) * sprite.color;
}

fn ribbon_texture(sprite: Ribbon) -> vec4<f32> {
    // XRack wireframe discard
    return sprite_sample(sprite.uv, sprite.sheet, sprite.frames) * sprite.color;
}

@fragment
fn fragment_trail_texture(sprite: Ribbon) -> @location(0) vec4<f32> {
    return ribbon_texture(sprite);
}

@fragment
fn fragment_trail_texture_bounded(sprite: Ribbon) -> @location(0) vec4<f32> {
    return bounded(ribbon_texture(sprite));
}

@fragment
fn fragment_trail_texture_weighted(sprite: Ribbon /* XRack linked sample */) -> WeightedColor {
    let color = ribbon_texture(sprite);
    // XRack linked fragment sprite.position
    return weighted_color(color, sprite.depth);
}

@fragment
fn fragment_texture(sprite: Sprite) -> @location(0) vec4<f32> {
    return texture_color(sprite);
}

fn bounded(color: vec4<f32>) -> vec4<f32> {
    // The screen and multiply factors describe a filter, so values above
    // one in straight RGB must not invert or amplify the destination.
    return vec4<f32>(min(color.rgb, vec3<f32>(color.a)), color.a);
}

@fragment
fn fragment_bounded(sprite: Sprite) -> @location(0) vec4<f32> {
    return bounded(sprite_color(sprite));
}

@fragment
fn fragment_texture_bounded(sprite: Sprite) -> @location(0) vec4<f32> {
    return bounded(texture_color(sprite));
}

struct WeightedColor {
    @location(0) accumulation: vec4<f32>,
    @location(1) opacity: f32,
}

@fragment
fn fragment_weighted(sprite: Sprite /* XRack linked sample */) -> WeightedColor {
    let color = sprite_color(sprite);
    // XRack linked fragment sprite.position
    return weighted_color(color, sprite.depth);
}

@fragment
fn fragment_texture_weighted(sprite: Sprite /* XRack linked sample */) -> WeightedColor {
    let color = texture_color(sprite);
    // XRack linked fragment sprite.position
    return weighted_color(color, sprite.depth);
}

fn weighted_color(color: vec4<f32>, view_depth: f32) -> WeightedColor {
    let depth = clamp(view_depth * view.lens.z, 0.0, 128.0);
    let importance = (0.25 + 0.75 * color.a) / (1.0 + depth * depth);
    let weight = max(importance, 0.0078125) * 0.03125;
    return WeightedColor(color * weight, color.a);
}
