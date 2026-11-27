struct ModelBlend {
    // Time, flight duration, stagger interval, arrival interval.
    clock: vec4<f32>,
    control: vec4<u32>,
    origin: vec4<f32>,
    offset: vec4<f32>,
    velocity: vec4<f32>,
    spread: vec4<f32>,
    gravity: vec4<f32>,
    spin: vec4<f32>,
    destination: vec4<f32>,
    scale: vec4<f32>,
    sweep: vec4<f32>,
    sweep_range: vec4<f32>,
    destination_x: vec4<f32>,
    destination_y: vec4<f32>,
    destination_z: vec4<f32>,
}

@group(0) @binding(0) var<storage, read> rest_vertices: array<u32>;
@group(0) @binding(1) var<storage, read_write> posed_vertices: array<u32>;
@group(0) @binding(2) var<uniform> blend: ModelBlend;
@group(0) @binding(3) var<storage, read> rest_tangents: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read_write> posed_tangents: array<vec2<u32>>;

const VERTEX_WORDS = 14u;
const EXPLODE = 1u;
const CONSTRUCT = 2u;
override AFFINE_DESTINATION: bool = false;

fn random01(input: u32) -> f32 {
    var bits = (input ^ (input >> 16u)) * 0x7feb352du;
    bits = (bits ^ (bits >> 15u)) * 0x846ca68bu;
    return f32((bits ^ (bits >> 16u)) >> 8u) / 16777216.0;
}

fn load_vector(word: u32) -> vec3<f32> {
    return bitcast<vec3<f32>>(vec3<u32>(
        rest_vertices[word],
        rest_vertices[word + 1u],
        rest_vertices[word + 2u],
    ));
}

fn store_vector(word: u32, xyz: vec3<f32>) {
    let bits = bitcast<vec3<u32>>(xyz);
    posed_vertices[word] = bits.x;
    posed_vertices[word + 1u] = bits.y;
    posed_vertices[word + 2u] = bits.z;
}

fn rotate(q: vec4<f32>, v: vec3<f32>) -> vec3<f32> {
    let twice = 2.0 * cross(q.xyz, v);
    return v + q.w * twice + cross(q.xyz, twice);
}

struct FragmentPose {
    center: vec3<f32>,
    position: vec3<f32>,
    orientation: vec4<f32>,
    scale: vec3<f32>,
    // 0: rest, 1: collapsed before birth, 2: in flight, 3: landed, 4: affine arrival.
    stage: u32,
    affine_progress: f32,
}

fn fragment_pose(triangle: u32) -> FragmentPose {
    let first = triangle * 3u * VERTEX_WORDS;
    let center = (load_vector(first) + load_vector(first + VERTEX_WORDS)
        + load_vector(first + 2u * VERTEX_WORDS)) / 3.0;
    let seed = blend.control.z ^ (triangle * 0x9e3779b9u);
    let mode = blend.control.x;
    let projected = dot(center, blend.sweep.xyz);
    var release = (f32(triangle) / f32(max(1u, blend.control.w - 1u))) * blend.clock.z;
    switch blend.control.y {
        case 1u: { release = random01(seed ^ 0xa24baed5u) * blend.clock.z; }
        case 2u: {
            release = clamp((projected - blend.sweep.w) * blend.sweep_range.x, 0.0, 1.0)
                * blend.clock.z;
        }
        case 3u: {
            let distance = projected + blend.sweep.w;
            // A stationary plane leaves its negative half inactive.
            if distance <= 0.0 && blend.sweep_range.x == 0.0 {
                release = blend.clock.x + 1.0;
            } else {
                release = max(-distance, 0.0) * blend.sweep_range.x;
            }
        }
        default: {}
    }
    let elapsed = blend.clock.x - release;
    if elapsed <= 0.0 {
        let stage = select(0u, 1u, mode == CONSTRUCT);
        return FragmentPose(center, blend.origin.xyz, vec4<f32>(0,0,0,1), vec3<f32>(1), stage, 0.0);
    }
    let duration = blend.clock.y;
    if elapsed >= duration && mode != EXPLODE {
        if AFFINE_DESTINATION {
            return FragmentPose(vec3<f32>(0), blend.offset.xyz, vec4<f32>(0,0,0,1), vec3<f32>(1), 4u, 1.0);
        }
        return FragmentPose(vec3<f32>(0), blend.offset.xyz, blend.destination, blend.scale.xyz, 3u, 0.0);
    }
    let age = min(elapsed, duration);
    let variation = 2.0 * vec3<f32>(
        random01(seed), random01(seed ^ 0xc13fa9a9u), random01(seed ^ 0x91e10da5u)) - 1.0;
    let velocity = blend.velocity.xyz + variation * blend.spread.xyz;
    let origin = select(center, blend.origin.xyz, mode == CONSTRUCT);
    let traveled = origin + age * (velocity + (0.5 * age) * blend.gravity.xyz);
    let half_turn = 3.14159265359 * fract(age * blend.spin.w / 6.28318530718);
    let spin = vec4<f32>(blend.spin.xyz * sin(half_turn), cos(half_turn));
    let fraction = clamp((age - (duration - blend.clock.w)) / blend.clock.w, 0.0, 1.0);
    let settle = select(fraction * fraction * (3.0 - 2.0 * fraction), 0.0, mode == EXPLODE);
    if AFFINE_DESTINATION && settle > 0.0 {
        let landing = destination_basis() * center + blend.offset.xyz;
        return FragmentPose(center, mix(traveled, landing, settle), spin, vec3<f32>(1), 4u, settle);
    }
    let end_rotation = select(blend.destination, -blend.destination, dot(spin, blend.destination) < 0.0);
    let landing = rotate(end_rotation, center * blend.scale.xyz) + blend.offset.xyz;
    return FragmentPose(center, mix(traveled, landing, settle), normalize(mix(spin, end_rotation, settle)),
        mix(vec3<f32>(1), blend.scale.xyz, settle), 2u, 0.0);
}

fn destination_basis() -> mat3x3<f32> {
    return mat3x3<f32>(blend.destination_x.xyz, blend.destination_y.xyz, blend.destination_z.xyz);
}

struct FragmentBasis {
    linear: mat3x3<f32>,
    normal: mat3x3<f32>,
    reflected: bool,
}

fn fragment_basis(pose: FragmentPose) -> FragmentBasis {
    if AFFINE_DESTINATION && pose.stage == 4u {
        let spin = mat3x3<f32>(
            rotate(pose.orientation, vec3<f32>(1,0,0)),
            rotate(pose.orientation, vec3<f32>(0,1,0)),
            rotate(pose.orientation, vec3<f32>(0,0,1)),
        );
        let basis = spin * (1.0 - pose.affine_progress) + destination_basis() * pose.affine_progress;
        let normal = mat3x3<f32>(cross(basis[1], basis[2]), cross(basis[2], basis[0]), cross(basis[0], basis[1]));
        let reflected = dot(basis[0], normal[0]) < 0.0;
        return FragmentBasis(basis, normal * select(1.0, -1.0, reflected), reflected);
    }
    let identity = mat3x3<f32>(vec3<f32>(1,0,0), vec3<f32>(0,1,0), vec3<f32>(0,0,1));
    return FragmentBasis(identity, identity, false);
}

fn original_corner(corner: u32, basis: FragmentBasis) -> u32 {
    return select(corner, 3u - corner, AFFINE_DESTINATION && basis.reflected && corner != 0u);
}

fn posed_corner(source: u32, destination: u32, pose: FragmentPose, basis: FragmentBasis) {
    let first = source * VERTEX_WORDS;
    let output_start = destination * VERTEX_WORDS;
    for (var i = 0u; i < VERTEX_WORDS; i++) {
        posed_vertices[output_start + i] = rest_vertices[first + i];
    }
    if pose.stage == 1u {
        store_vector(output_start, pose.position);
    } else if AFFINE_DESTINATION && pose.stage == 4u {
        store_vector(output_start, pose.position + basis.linear * (load_vector(first) - pose.center));
        let normal = load_vector(first + 3u);
        let transformed = basis.normal * normal;
        // Cofactors remain finite at a mirror crossing. Use the spin normal
        // when the affine blend collapses this particular normal to zero.
        store_vector(output_start + 3u,
            select(rotate(pose.orientation, normal), transformed, dot(transformed, transformed) > 1e-16));
    } else if pose.stage >= 2u {
        let local = (load_vector(first) - pose.center) * pose.scale;
        store_vector(output_start, pose.position + rotate(pose.orientation, local));
        store_vector(output_start + 3u, rotate(pose.orientation, load_vector(first + 3u) / pose.scale));
    }
}

@compute @workgroup_size(64)
fn deform(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= blend.control.w { return; }
    let pose = fragment_pose(id.x);
    let basis = fragment_basis(pose);
    for (var corner = 0u; corner < 3u; corner++) {
        posed_corner(3u * id.x + original_corner(corner, basis), 3u * id.x + corner, pose, basis);
    }
}

@compute @workgroup_size(64)
fn deform_tangent(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= blend.control.w { return; }
    let pose = fragment_pose(id.x);
    let basis = fragment_basis(pose);
    for (var corner = 0u; corner < 3u; corner++) {
        let vertex = 3u * id.x + corner;
        let source = 3u * id.x + original_corner(corner, basis);
        posed_corner(source, vertex, pose, basis);
        let packed = rest_tangents[source];
        var output = packed;
        if pose.stage >= 2u {
            let xy = unpack2x16snorm(packed.x);
            let zw = unpack2x16snorm(packed.y);
            let original = vec3<f32>(xy, zw.x);
            var direction = rotate(pose.orientation, original * pose.scale);
            var handedness = zw.y;
            if AFFINE_DESTINATION && pose.stage == 4u {
                direction = basis.linear * original;
                handedness *= select(1.0, -1.0, basis.reflected);
            }
            let len2 = dot(direction, direction);
            if len2 > 0.0 {
                let tangent = direction * inverseSqrt(len2);
                output = vec2<u32>(pack2x16snorm(tangent.xy), pack2x16snorm(vec2<f32>(tangent.z, handedness)));
            }
        }
        posed_tangents[vertex] = output;
    }
}
