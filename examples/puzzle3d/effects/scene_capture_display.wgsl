fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let parameters = scene_effect_parameters();
    let right = uv.x >= 0.5;
    if right != (parameters.x > 0.5) || uv.y >= 0.5 {
        return scene_color(uv);
    }
    if !scene_has_buffer(0u) {
        return vec4(0.055, 0.065, 0.085, 1.0);
    }
    let feed = scene_buffer_color(0u, vec2(fract(uv.x * 2.0), uv.y * 2.0));
    let paper = vec3(0.035, 0.05, 0.08);
    return vec4(mix(paper, feed.rgb * parameters.y, feed.a), 1.0);
}
