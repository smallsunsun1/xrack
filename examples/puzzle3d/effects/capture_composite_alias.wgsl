fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if scene_effect_parameters().z > 1.0 {
        if uv.x < 0.5 || !scene_buffer_world_position(1u, uv).valid {
            return scene_color(uv);
        }
        let data = scene_buffer_color(0u, uv);
        return vec4(data.r, data.g, select(0.0, data.b + 0.15, data.a < 0.0), 1.0);
    }
    if scene_effect_parameters().z > 0.0 {
        let pixel = scene_buffer_world_position(1u, uv);
        if uv.x < 0.5 || !pixel.valid {
            return scene_color(uv);
        }
        let encoded_normal = scene_buffer_color(0u, uv).xyz * 0.5 + vec3(0.5);
        let base = scene_buffer_color(2u, uv);
        let emissive = scene_buffer_color(3u, uv);
        let depth = scene_buffer_color(1u, uv).r;
        let color = encoded_normal * (0.55 + 0.15 * depth)
            + base.rgb * 0.15 + emissive.rgb * 0.12 + vec3(base.a + emissive.a) * 0.04;
        return vec4(color, 1.0);
    }
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
