fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if scene_effect_parameters().x > 0.5 {
        // One quarter of a half-float ULP above 0.5.
        return vec4(0.5001220703125, -0.5, 3.0, 0.5);
    }
    let column = u32(uv.x * scene_size().x);
    let stripe = select(0.125, 0.875, column % 8u >= 4u);
    return vec4(stripe, -0.5, 3.0, 0.5);
}
