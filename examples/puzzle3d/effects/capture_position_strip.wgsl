fn position_at(slot: u32, uv: vec2<f32>) -> XrCapturePosition {
    return scene_buffer_world_position(slot, uv);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if uv.y >= 0.20 {
        return scene_color(uv);
    }
    let center = vec2(0.5);
    if position_at(2u, center).valid || position_at(3u, center).valid
        || position_at(99u, center).valid || position_at(0u, vec2(0.02)).valid {
        return vec4(1.0, 0.0, 0.0, 1.0);
    }
    let slot = select(0u, 1u, uv.x >= 0.5);
    let point = position_at(slot, center);
    if point.valid {
        return vec4(point.position * vec3(0.13, 0.125, 0.16) + vec3(0.5, 0.5, 0.0), 1.0);
    }
    return vec4(0.035, 0.055, 0.07, 1.0);
}
