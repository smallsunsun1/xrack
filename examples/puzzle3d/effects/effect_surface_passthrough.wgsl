fn unused_visualization(uv: vec2<f32>) -> vec4<f32> {
    return vec4(scene_surface(uv).normal * 0.5 + 0.5, 1.0);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    return scene_color(uv);
}
