fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if scene_effect_parameters().y > 0.0 {
        return vec4(vec3(scene_buffer_color(0u, uv).a), 1.0);
    }
    if scene_effect_parameters().x > 0.0 {
        let point = scene_buffer_world_position(0u, uv);
        return vec4(
            scene_buffer_color(0u, uv).x,
            select(0.2, 0.8, point.valid),
            select(0.1, 0.6, point.valid),
            1.0,
        );
    }
    let a = scene_buffer_color(0u, uv);
    let b = scene_buffer_color(1u, uv);
    if !scene_has_buffer(0u) || !scene_has_buffer(1u) || any(abs(a - b) > vec4(0.002)) {
        return vec4(1.0, 0.0, 1.0, 1.0);
    }
    return scene_color(uv);
}
