fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if scene_has_volume(0u) && scene_buffer_volume_size(0u).z > 0u {
        let sampled = scene_buffer_volume(0u, scene_effect_parameters());
        return vec4(sampled.rgb * sampled.a, 1.0);
    }
    return vec4(scene_buffer_color(1u, vec2(0.35, 0.5)).rgb, 1.0);
}
