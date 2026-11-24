fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let surface = scene_surface(uv);
    return vec4(select(0.0, surface.roughness, surface.valid), 0.0, 0.0, 1.0);
}
