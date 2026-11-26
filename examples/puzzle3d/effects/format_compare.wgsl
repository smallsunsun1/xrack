fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let sample_uv = vec2(fract(uv.x * 2.0) * 1.7 - 0.35, uv.y);
    let side = select(1u, 0u, uv.x < 0.5);
    let data = scene_buffer_color(side, sample_uv);
    if scene_effect_parameters().x > 0.5 {
        return vec4(clamp((data.r - 0.5) * 4096.0, 0.0, 1.0), 0.0, 0.0, 1.0);
    }
    let red = data.r * 0.8 + 0.03;
    let green = max(-data.g, 0.0) * 0.85 + 0.04;
    let blue = clamp(data.b / 3.0, 0.0, 1.0) * 0.3 + data.r * 0.45;
    return vec4(red, green, blue, 1.0);
}
