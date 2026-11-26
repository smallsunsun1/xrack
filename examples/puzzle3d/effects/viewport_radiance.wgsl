fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    // Straight scene-linear color with 50% coverage and overbrights.
    return vec4<f32>(1.25, 0.25, 0.02, 0.5);
}
