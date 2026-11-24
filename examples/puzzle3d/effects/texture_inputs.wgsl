fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let column = u32(uv.x * 4.0);
    if column >= 3u {
        return scene_original_color(uv);
    }
    let color = scene_buffer_color(column, vec2(0.35, 0.5));
    return vec4(color.rgb, select(0.0, 1.0, scene_has_buffer(column)));
}
