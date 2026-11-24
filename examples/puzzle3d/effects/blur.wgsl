fn premultiplied(color: vec4<f32>) -> vec4<f32> {
    return vec4(color.rgb * color.a, color.a);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let offset = scene_effect_parameters().xy / scene_input_size();
    let filtered = 0.4 * premultiplied(scene_color(uv))
        + 0.24 * (premultiplied(scene_color(uv + offset))
                  + premultiplied(scene_color(uv - offset)))
        + 0.06 * (premultiplied(scene_color(uv + 2.0 * offset))
                  + premultiplied(scene_color(uv - 2.0 * offset)));
    return vec4(filtered.rgb / max(filtered.a, 0.000001), filtered.a);
}
