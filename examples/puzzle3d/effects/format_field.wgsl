fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let column = u32(uv.x * scene_size().x);
    let stripe = select(0.125, 0.875, column % 8u >= 4u);
    return vec4(stripe, -0.5, 3.0, 0.5);
}
