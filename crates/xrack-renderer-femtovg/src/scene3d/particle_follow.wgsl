struct FollowJob {
    indices: vec4<u32>,
    timing: vec4<f32>,
}

@group(0) @binding(XRACK_FOLLOW_BINDING) var<storage, read_write> follow_output: array<vec4<f32>>;
@group(0) @binding(XRACK_JOB_BINDING) var<storage, read> follow_jobs: array<FollowJob>;

@compute @workgroup_size(64)
fn compute_follow(@builtin(global_invocation_id) id: vec3<u32>) {
    if id.x >= arrayLength(&follow_jobs) {
        return;
    }
    let job = follow_jobs[id.x];
    let birth = XRACK_FOLLOW_SOURCE(job.indices.x);
    let stride = select(1u, 3u, FOLLOW_SURFACE_SOURCE);
    let direction = normalized_or(birth.velocity, vec3<f32>(0.0));
    for (var local = 0u; local < job.indices.y; local++) {
        let age = min(birth.particle.emitter.spread_lifetime.w,
            max(0.0, job.timing.x + f32(local) * job.timing.y));
        let source = trajectory(birth, age);
        let address = (job.indices.z + local) * stride;
        follow_output[address] = vec4<f32>(XRACK_FOLLOW_CENTER, 0.0);
        if FOLLOW_SURFACE_SOURCE {
            follow_output[address + 1u] = vec4<f32>(birth.normal, 0.0);
            follow_output[address + 2u] = vec4<f32>(direction, 0.0);
        }
    }
}
