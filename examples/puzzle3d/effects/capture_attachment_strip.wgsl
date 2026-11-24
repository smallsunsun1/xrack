fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if uv.y >= 0.26 {
        return scene_color(uv);
    }
    let at = vec2(fract(uv.x * 2.0), uv.y / 0.26);
    if uv.x < 0.5 {
        return vec4(vec3(scene_buffer_color(0u, at).x), 1.0);
    }
    let surface = scene_buffer_color(1u, at);
    let data = vec3(surface.a, abs(surface.y), abs(surface.z));
    return vec4(data, 1.0);
}
