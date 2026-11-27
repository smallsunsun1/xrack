fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if !scene_has_buffer(0u) {
        return scene_original_color(uv);
    }
    let texel = scene_buffer_color(0u, uv);
    return mix(scene_original_color(uv), vec4(texel.rgb, 1.0), texel.a);
}
