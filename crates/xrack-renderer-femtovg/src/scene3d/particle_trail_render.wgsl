@group(0) @binding(XRACK_TRAIL_BINDING) var<storage, read> trail_points: array<TrailPoint>;

fn prepared_trail(vertex: u32, index: u32) -> SpriteVertex {
    let particle = particle_at(index);
    let segments = trails.segment_count.x;
    let base = particle_source_index(index) * (segments + 1u + XRACK_TRAIL_HEADER_ROWS);
    let first = base + XRACK_TRAIL_HEADER_ROWS;
    var start_velocity = vec3<f32>(0.0);
    if XRACK_TRAIL_HEADER_ROWS != 0u {
        start_velocity = trail_points[base].center_age.xyz;
    }
    let point = vertex / 2u;
    let younger = trail_points[first + max(point, 1u) - 1u].center_age.xyz;
    let older = trail_points[first + min(point + 1u, segments)].center_age.xyz;
    return trail_point_vertex(vertex, particle, trail_points[first].center_age.w,
        trail_points[first + point], younger, older, start_velocity);
}

@vertex
fn vertex_prepared_trail(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(prepared_trail(vertex, index).sprite);
}

@vertex
fn vertex_prepared_trail_tinted(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> Ribbon {
    return ribbon(tinted(prepared_trail(vertex, index)));
}
